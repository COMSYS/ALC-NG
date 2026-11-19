//! Values of custom conditionals (`\newif`) across files, traced while compiling.
//!
//! The cleaner evaluates custom conditionals while it walks a file, but each file is cleaned on
//! its own. A conditional is often defined and set in one file and used in another one, which is
//! loaded with `\input` or `\include`, possibly several times with different values. It may also
//! be set by a command defined elsewhere (e.g. `\cvprfinalcopy` of a template), or be used in
//! the body of a definition, which is executed later.
//!
//! TeX knows these values. Before compiling, a probe is loaded at the start of each main file.
//! It writes the events below into `<jobname>.alcng-ifs`:
//!
//! ```text
//! before 3 :config:settings.tex
//! set 2 0 anonymous=0 :config:settings.tex
//! set 3 0 anonymous=1 :config:settings.tex
//! after 3 anonymous=1 draft=0 :config:settings.tex
//! document 12 1 anonymous=1 draft=0 :./:main.tex
//! ```
//!
//! - `before` and `after`: a file is loaded or ends (`file/before` and `file/after` hooks of
//!   LaTeX 2020-10 and later), with the values of the defined conditionals.
//! - `set`: a conditional is defined (`\newif`, value `0`) or set with its `\...true` or
//!   `\...false` command, with the group level (`\currentgrouplevel`) of the assignment.
//! - `document`: `\begin{document}`, with the group level and the values.
//!
//! Each line holds the event, the line of the file that is read (`\inputlineno`) and the path
//! and name of that file (`\CurrentFilePath`, `\CurrentFile`). Files loaded with the primitive
//! syntax (`\input file`) do not trigger the hooks; the values in such files are unknown.
//!
//! The events form a timeline of the run. A conditional in a definition is evaluated if its
//! value is the same whenever the definition may be executed, see
//! [`FileIfState::value_in_definition`].

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use log::{debug, trace};

/// Name of the probe file, which is placed next to each main file in the cache.
pub const PROBE_NAME: &str = "alc-ng-if-probe";
/// Extension of the trace file written by the probe, next to the main file.
pub const TRACE_EXTENSION: &str = "alcng-ifs";

/// The value of a conditional at a point of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IfValue {
    Known(bool),
    /// The conditional is defined, but its value is not known, e.g. as the file is loaded
    /// with different values. The conditional is kept and both branches are cleaned.
    Unknown,
}

/// The values of conditionals by their name (without the `\if` prefix).
pub type IfValues = HashMap<Vec<u8>, IfValue>;

/// The values of the conditionals in a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileIfState {
    /// Values at the start of the file.
    pub entry: IfValues,
    /// Values after a file loaded from this file returned or after a conditional was set, with
    /// the (1-based) line of this file where it happened. Ordered as in the file.
    pub returns: Vec<(usize, IfValues)>,
    /// Where the file was read in the [`Timeline`], once per load.
    pub occurrences: Vec<Occurrence>,
    /// The values of the conditionals over the whole run.
    pub timeline: Arc<Timeline>,
}

impl FileIfState {
    /// The value of the conditional `name` in a definition that starts at `line` (1-based) of
    /// this file, i.e. its value whenever the definition may be executed.
    ///
    /// The value is known if it does not change from the definition to the end of the run.
    /// If the defined commands (`commands_used_in_preamble` is false) are only used after
    /// `\begin{document}`, it suffices that the value does not change in the document.
    ///
    /// Returns `None` if the conditional is not traced at all.
    pub fn value_in_definition(
        &self,
        name: &[u8],
        line: usize,
        commands_used_in_preamble: bool,
    ) -> Option<IfValue> {
        if !self.timeline.is_traced(name) {
            return None;
        }

        let mut values = self.occurrences.iter().map(|occurrence| {
            self.timeline.value_in_definition(
                name,
                occurrence.point_before(line),
                commands_used_in_preamble,
            )
        });

        // A file that is not traced or loaded several times with different values
        let first = values.next().unwrap_or(IfValue::Unknown);
        Some(if values.all(|value| value == first) {
            first
        } else {
            IfValue::Unknown
        })
    }

    /// Whether the command (with backslash) is used in the preamble of a project file, outside
    /// of its definition.
    pub fn is_used_in_preamble(&self, command: &[u8]) -> bool {
        self.timeline.preamble_commands.contains(command)
    }
}

/// Where a file was read in the run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Occurrence {
    /// Point of the timeline when the file was loaded.
    entry: usize,
    /// Points of the events while the file was read, with the line of this file.
    events: Vec<(usize, usize)>,
}

impl Occurrence {
    /// The last point in front of `line` (1-based). An event on the line itself may happen
    /// before or after a construct on the line, thus it counts as after.
    fn point_before(&self, line: usize) -> usize {
        match self.events.partition_point(|(l, _)| *l < line) {
            0 => self.entry,
            i => self.events[i - 1].1,
        }
    }
}

/// A run of LaTeX on a main file, as range of points of the [`Timeline`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    start: usize,
    end: usize,
    /// Point of `\begin{document}`.
    document: Option<usize>,
}

/// The values of the traced conditionals over all runs. Each event of the trace is a point.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timeline {
    /// Each change of the value of a conditional, ordered by point.
    changes: HashMap<Vec<u8>, Vec<(usize, IfValue)>>,
    runs: Vec<Run>,
    /// Commands (with backslash) used in the preamble of a project file, see
    /// [`IfTrace::add_preamble_commands`].
    preamble_commands: HashSet<Vec<u8>>,
    /// Conditionals that are assigned without their `\...true` or `\...false` command
    /// (e.g. `\let\ifdraft\iftrue`), which the probe does not see.
    untraced_assignments: HashSet<Vec<u8>>,
}

impl Timeline {
    fn is_traced(&self, name: &[u8]) -> bool {
        self.changes.contains_key(name)
    }

    fn record(&mut self, name: &[u8], point: usize, value: IfValue) {
        let changes = self.changes.entry(name.to_vec()).or_default();
        if changes.last().is_none_or(|(_, last)| *last != value) {
            changes.push((point, value));
        }
    }

    /// The value of a conditional in a definition at `point`, see
    /// [`FileIfState::value_in_definition`].
    fn value_in_definition(
        &self,
        name: &[u8],
        point: usize,
        commands_used_in_preamble: bool,
    ) -> IfValue {
        let Some(run) = self
            .runs
            .iter()
            .find(|run| run.start <= point && point <= run.end)
        else {
            return IfValue::Unknown;
        };
        if self.untraced_assignments.contains(name) {
            return IfValue::Unknown;
        }

        match self.constant_from(name, point, run) {
            IfValue::Known(value) => IfValue::Known(value),
            IfValue::Unknown => match run.document {
                Some(document) if point < document && !commands_used_in_preamble => {
                    self.constant_from(name, document, run)
                }
                _ => IfValue::Unknown,
            },
        }
    }

    /// The value of a conditional if it stays the same from `point` to the end of the run.
    fn constant_from(&self, name: &[u8], point: usize, run: &Run) -> IfValue {
        let Some(changes) = self.changes.get(name) else {
            return IfValue::Unknown;
        };

        let start = changes.partition_point(|(p, _)| *p <= point);
        let current = start
            .checked_sub(1)
            .map(|i| changes[i])
            .filter(|(p, _)| *p >= run.start);
        // A conditional defined after the definition: the command cannot be used before
        let mut values = current
            .into_iter()
            .chain(changes[start..].iter().copied())
            .take_while(|(p, _)| *p <= run.end)
            .map(|(_, value)| value);

        match values.next() {
            Some(IfValue::Known(first)) if values.all(|v| v == IfValue::Known(first)) => {
                IfValue::Known(first)
            }
            _ => IfValue::Unknown,
        }
    }
}

/// The values of the conditionals in all files of the project, see the module documentation.
#[derive(Debug, Default)]
pub struct IfTrace {
    /// All traced conditionals.
    names: Vec<Vec<u8>>,
    /// Each time a file is loaded, by the canonical path of the file.
    files: HashMap<PathBuf, Vec<(FileIfState, Occurrence)>>,
    timeline: Arc<Timeline>,
    /// The next point of the timeline.
    next_point: usize,
    /// Whether the trace of every main file was read.
    complete: bool,
}

impl IfTrace {
    /// Creates a trace of the given conditionals, which is complete until a main file is missing.
    pub fn new(names: impl IntoIterator<Item = Vec<u8>>) -> Self {
        let mut names: Vec<_> = names
            .into_iter()
            .collect::<HashSet<_>>()
            .into_iter()
            .filter(|name| is_traceable(name))
            .collect();
        names.sort();

        Self {
            names,
            complete: true,
            ..Default::default()
        }
    }

    /// The conditionals written by the probe.
    pub fn names(&self) -> &[Vec<u8>] {
        &self.names
    }

    /// Whether the values of each main file are known.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Marks the trace as incomplete, e.g. as the probe did not run for a main file.
    pub fn mark_incomplete(&mut self) {
        self.complete = false;
    }

    /// The TeX source of the probe, which writes the values of the traced conditionals.
    pub fn probe(&self) -> String {
        let names = || self.names.iter().map(|name| String::from_utf8_lossy(name));
        let values: String = names()
            .map(|name| format!("\\alcng@value{{{name}}}"))
            .collect();
        let traced: String = names()
            .map(|name| format!("\\expandafter\\let\\csname alcng@t@{name}\\endcsname\\@empty\n"))
            .collect();
        let defined: String = names()
            .map(|name| format!("\\alcng@init{{{name}}}"))
            .collect();

        // The probe stops without file hooks (LaTeX older than 2020-10) and if it is loaded
        // again, e.g. as a main file is loaded by another one.
        // `\ifx` compares the meaning of a conditional with `\iftrue` and `\iffalse` without
        // evaluating it. The comparisons are split into macros, as `\iftrue` and `\iffalse` in
        // skipped branches would count as nested conditionals.
        //
        // The `\...true` and `\...false` commands of the traced conditionals additionally write
        // a `set` event. The assignment comes first, so `\global\draftfalse` stays global, and
        // the event is written immediately and `\protected`, so it adds nothing to the page and
        // is not expanded in `\edef`. `\newif` is extended to do the same for conditionals
        // defined later; those defined before the probe are changed right away.
        format!(
            r"\ifcsname AddToHook\endcsname\else\expandafter\endinput\fi
\ifcsname alcng@out\endcsname\expandafter\endinput\fi
\makeatletter
\newwrite\alcng@out
\immediate\openout\alcng@out=\jobname.{TRACE_EXTENSION}\relax
\def\alcng@value#1{{\ifcsname if#1\endcsname\expandafter\alcng@defined\else\expandafter\@gobble\fi{{#1}}}}
\def\alcng@defined#1{{\expandafter\ifx\csname if#1\endcsname\iftrue\alcng@append{{#1}}{{1}}\else\alcng@definedfalse{{#1}}\fi}}
\def\alcng@definedfalse#1{{\expandafter\ifx\csname if#1\endcsname\iffalse\alcng@append{{#1}}{{0}}\fi}}
\def\alcng@append#1#2{{\edef\alcng@values{{\alcng@values\space#1=#2}}}}
\def\alcng@location{{:\CurrentFilePath:\CurrentFile}}
\def\alcng@event#1#2{{\def\alcng@values{{}}{values}\immediate\write\alcng@out{{#1 \the\inputlineno#2\alcng@values\space\alcng@location}}}}
\protected\def\alcng@set#1#2{{\immediate\write\alcng@out{{set \the\inputlineno\space\the\currentgrouplevel\space#1=#2 \alcng@location}}}}
{traced}\def\alcng@patch#1{{\ifcsname alcng@t@#1\endcsname\expandafter\alcng@setters\else\expandafter\@gobble\fi{{#1}}}}
\def\alcng@setters#1{{%
\expandafter\def\csname#1true\endcsname{{\expandafter\let\csname if#1\endcsname\iftrue\alcng@set{{#1}}{{1}}}}%
\expandafter\def\csname#1false\endcsname{{\expandafter\let\csname if#1\endcsname\iffalse\alcng@set{{#1}}{{0}}}}}}
\def\alcng@newpatch#1{{\ifcsname alcng@t@#1\endcsname\expandafter\alcng@new\else\expandafter\@gobble\fi{{#1}}}}
\def\alcng@new#1{{\alcng@setters{{#1}}\alcng@set{{#1}}{{0}}}}
\let\alcng@newif\newif
\def\newif#1{{\alcng@newif#1\begingroup\escapechar\m@ne\edef\alcng@name{{\endgroup\noexpand\alcng@newpatch{{\expandafter\@gobbletwo\string#1}}}}\alcng@name}}
\def\alcng@init#1{{\ifcsname if#1\endcsname\expandafter\alcng@initdefined\else\expandafter\@gobble\fi{{#1}}}}
\def\alcng@initdefined#1{{\alcng@setters{{#1}}\expandafter\ifx\csname if#1\endcsname\iftrue\alcng@set{{#1}}{{1}}\else\alcng@initfalse{{#1}}\fi}}
\def\alcng@initfalse#1{{\expandafter\ifx\csname if#1\endcsname\iffalse\alcng@set{{#1}}{{0}}\fi}}
{defined}
\AddToHook{{file/before}}{{\alcng@event{{before}}{{}}}}
\AddToHook{{file/after}}{{\alcng@event{{after}}{{}}}}
\AddToHook{{begindocument/before}}{{\alcng@event{{document}}{{\space\the\currentgrouplevel}}}}
\makeatother
"
        )
    }

    /// Reads the trace written by the probe for the main file at `main` (a canonical path).
    pub fn add_main(&mut self, main: &Path, trace: &str) {
        let base = main.parent().unwrap_or(main);
        let timeline = Arc::make_mut(&mut self.timeline);
        let start = self.next_point;

        // The occurrences of the files and the stack of currently loaded files.
        let mut occurrences = vec![(
            Some(main.to_path_buf()),
            FileIfState::default(),
            Occurrence {
                entry: start,
                events: Vec::new(),
            },
        )];
        let main_name = main
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut stack = vec![(0, main_name)];
        // Assignments in a group deeper than this level are undone at its end, which is not traced
        let mut base_level = 0;
        let mut document = None;

        for line in trace.lines() {
            let Some(event) = Event::parse(line) else {
                debug!("Ignoring malformed line of the conditional trace: {line:?}");
                continue;
            };
            let point = self.next_point;
            self.next_point += 1;
            let (top, top_file) = stack.last().expect("the main file is never removed");
            let top = *top;
            // `\CurrentFile` is empty in the main file. Events of the probe itself, before the
            // main file is read, have no line in it.
            let in_top =
                same_file(top_file, event.file) || (stack.len() == 1 && event.file.is_empty());

            match event.kind {
                EventKind::Before => {
                    let path = base.join(event.path).join(event.file);
                    let path = path.canonicalize().ok().filter(|p| p.is_file());

                    occurrences[top].2.events.push((event.line, point));
                    for (name, value) in &event.values {
                        timeline.record(name, point, *value);
                    }
                    occurrences.push((
                        path,
                        FileIfState {
                            entry: event.values,
                            ..Default::default()
                        },
                        Occurrence {
                            entry: point,
                            events: Vec::new(),
                        },
                    ));
                    stack.push((occurrences.len() - 1, event.file.to_owned()));
                }
                EventKind::After => {
                    for (name, value) in &event.values {
                        timeline.record(name, point, *value);
                    }

                    // Only the root (main file) remains or the event does not belong to the
                    // file on top, e.g. the probe itself, which is loaded before the hooks are set
                    match stack.last() {
                        Some((_, file)) if stack.len() > 1 && file == event.file => {
                            stack.pop();
                        }
                        _ => continue,
                    }

                    let (parent, _) = stack.last().expect("the main file is never removed");
                    occurrences[*parent].2.events.push((event.line, point));
                    occurrences[*parent]
                        .1
                        .returns
                        .push((event.line, event.values));
                }
                EventKind::Set => {
                    let values: IfValues = event
                        .values
                        .into_iter()
                        .map(|(name, value)| {
                            let value = if event.level > base_level {
                                IfValue::Unknown
                            } else {
                                value
                            };
                            timeline.record(&name, point, value);
                            (name, value)
                        })
                        .collect();

                    if in_top {
                        occurrences[top].2.events.push((event.line, point));
                        occurrences[top].1.returns.push((event.line, values));
                    }
                }
                EventKind::Document => {
                    base_level = event.level;
                    document = Some(point);
                    for (name, value) in &event.values {
                        timeline.record(name, point, *value);
                    }
                    if in_top {
                        occurrences[top].2.events.push((event.line, point));
                    }
                }
            }
        }

        timeline.runs.push(Run {
            start,
            end: self.next_point.saturating_sub(1).max(start),
            document,
        });

        for (path, state, occurrence) in occurrences {
            if let Some(path) = path {
                trace!(
                    "Traced conditionals of {}: {:?}",
                    path.display(),
                    state.entry
                );
                self.files
                    .entry(path)
                    .or_default()
                    .push((state, occurrence));
            }
        }
    }

    /// Whether `line` (1-based) of the file at `path` (a canonical path) may be read before
    /// `\begin{document}`. Files that are not traced may be read anywhere.
    fn is_preamble(&self, path: &Path, line: usize) -> bool {
        let Some(occurrences) = self.files.get(path) else {
            return true;
        };

        occurrences.iter().any(|(_, occurrence)| {
            let point = occurrence.point_before(line);
            self.timeline
                .runs
                .iter()
                .find(|run| run.start <= point && point <= run.end)
                .is_none_or(|run| run.document.is_none_or(|document| point < document))
        })
    }

    /// Scans a project file for commands used in the preamble and for assignments of
    /// conditionals that bypass the traced `\...true` and `\...false` commands.
    ///
    /// A conditional in a definition is only evaluated with its value in the document if the
    /// defined command is not used in the preamble, see [`FileIfState::value_in_definition`].
    /// The scan is textual and conservative: the name of a command after `\def`, `\newcommand`
    /// and the like is its definition, any other occurrence is a use.
    pub fn add_preamble_commands(&mut self, path: &Path, content: &[u8]) {
        let mut used = Vec::new();
        let mut assigned = Vec::new();

        for (index, line) in content.split(|&b| b == b'\n').enumerate() {
            let line = strip_comment(line);
            let preamble = self.is_preamble(path, index + 1);
            let mut i = 0;

            while i < line.len() {
                if line[i] != b'\\' {
                    i += 1;
                    continue;
                }
                let end = i
                    + 1
                    + line[i + 1..]
                        .iter()
                        .take_while(|b| b.is_ascii_alphabetic() || **b == b'@')
                        .count();
                if end == i + 1 {
                    // A control symbol, e.g. `\\` or `\%`
                    i += 2;
                    continue;
                }

                let command = &line[i..end];
                if command == b"\\let"
                    && let Some(name) = let_conditional(&line[end..])
                {
                    assigned.push(name.to_vec());
                }
                if preamble && !is_declaration(&line[..i]) {
                    used.push(command.to_vec());
                }
                i = end;
            }
        }

        let timeline = Arc::make_mut(&mut self.timeline);
        timeline.preamble_commands.extend(used);
        timeline.untraced_assignments.extend(assigned);
    }

    /// The values of the conditionals in the file at `path` (a canonical path).
    ///
    /// Returns `None` if the trace is not complete, then the values are not known at all.
    /// A file that is not traced (e.g. loaded with `\input file`) has unknown values.
    pub fn state(&self, path: &Path) -> Option<FileIfState> {
        if !self.complete {
            return None;
        }

        match self.files.get(path) {
            Some(occurrences) => {
                let states: Vec<_> = occurrences.iter().map(|(s, _)| s.clone()).collect();
                Some(FileIfState {
                    occurrences: occurrences.iter().map(|(_, o)| o.clone()).collect(),
                    timeline: self.timeline.clone(),
                    ..merge(&states)
                })
            }
            None => Some(FileIfState {
                entry: self
                    .names
                    .iter()
                    .map(|name| (name.clone(), IfValue::Unknown))
                    .collect(),
                returns: Vec::new(),
                occurrences: Vec::new(),
                timeline: self.timeline.clone(),
            }),
        }
    }
}

/// Whether `\CurrentFile` names the file, which may be given without extension.
fn same_file(file: &str, current: &str) -> bool {
    file == current
        || file.strip_suffix(".tex") == Some(current)
        || current.strip_suffix(".tex") == Some(file)
}

/// The line up to a comment.
fn strip_comment(line: &[u8]) -> &[u8] {
    let mut escaped = false;
    for (i, &b) in line.iter().enumerate() {
        match b {
            b'%' if !escaped => return &line[..i],
            b'\\' => escaped = !escaped,
            _ => escaped = false,
        }
    }
    line
}

/// Whether a command name following `before` is the name of a definition.
fn is_declaration(before: &[u8]) -> bool {
    const DEFINITIONS: [&[u8]; 9] = [
        br"\def",
        br"\gdef",
        br"\edef",
        br"\xdef",
        br"\newcommand",
        br"\renewcommand",
        br"\providecommand",
        br"\DeclareRobustCommand",
        br"\newrobustcmd",
    ];

    let before = before.trim_ascii_end();
    let before = before
        .strip_suffix(b"{")
        .map_or(before, <[u8]>::trim_ascii_end);
    let before = before.strip_suffix(b"*").unwrap_or(before);
    DEFINITIONS.iter().any(|d| before.ends_with(d))
}

/// The name of the conditional assigned by `\let` with the text after `\let`, e.g. `draft`
/// for `\let\ifdraft\iftrue`.
fn let_conditional(after: &[u8]) -> Option<&[u8]> {
    let name = after.trim_ascii_start().strip_prefix(br"\if")?;
    let len = name
        .iter()
        .take_while(|b| b.is_ascii_alphabetic() || **b == b'@')
        .count();
    (len > 0).then(|| &name[..len])
}

/// Inserts the probe at the start of the main file, without changing its line numbers.
pub fn inject_probe(content: &[u8]) -> Vec<u8> {
    let input = format!("\\input{{{PROBE_NAME}}}");

    // Keep a byte order mark and a format line (`%&pdflatex`) in front
    let mut position = if content.starts_with(b"\xEF\xBB\xBF") {
        3
    } else {
        0
    };
    if content[position..].starts_with(b"%&") {
        position = content[position..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(content.len(), |newline| position + newline + 1);
    }

    let mut injected = Vec::with_capacity(content.len() + input.len());
    injected.extend_from_slice(&content[..position]);
    injected.extend_from_slice(input.as_bytes());
    injected.extend_from_slice(&content[position..]);
    injected
}

/// Conditionals with other names cannot be written into the probe safely.
fn is_traceable(name: &[u8]) -> bool {
    !name.is_empty() && name.iter().all(|b| b.is_ascii_alphabetic() || *b == b'@')
}

/// Merges the states of a file that is loaded several times.
/// Values that differ between the occurrences are unknown.
fn merge(occurrences: &[FileIfState]) -> FileIfState {
    let Some((first, others)) = occurrences.split_first() else {
        return FileIfState::default();
    };

    let same_lines = others.iter().all(|other| {
        other.returns.len() == first.returns.len()
            && other
                .returns
                .iter()
                .zip(&first.returns)
                .all(|((a, _), (b, _))| a == b)
    });

    if same_lines {
        // Compare the values at each point of the file
        let entry = merge_values(occurrences.iter().map(|o| &o.entry));
        let returns = first
            .returns
            .iter()
            .enumerate()
            .map(|(i, (line, _))| {
                (
                    *line,
                    merge_values(occurrences.iter().map(|o| &o.returns[i].1)),
                )
            })
            .collect();
        return FileIfState {
            entry,
            returns,
            ..Default::default()
        };
    }

    // The files loaded from the file differ, thus a conditional that has different values
    // anywhere is unknown everywhere
    let all_values = || {
        occurrences
            .iter()
            .flat_map(|o| std::iter::once(&o.entry).chain(o.returns.iter().map(|(_, v)| v)))
    };
    let mut seen: HashMap<&[u8], HashSet<IfValue>> = HashMap::new();
    for values in all_values() {
        for (name, value) in values {
            seen.entry(name.as_slice()).or_default().insert(*value);
        }
    }
    let unstable = |values: &IfValues| {
        values
            .iter()
            .map(|(name, value)| match seen.get(name.as_slice()) {
                Some(seen) if seen.len() == 1 => (name.clone(), *value),
                _ => (name.clone(), IfValue::Unknown),
            })
            .collect::<IfValues>()
    };

    FileIfState {
        entry: unstable(&first.entry),
        returns: first
            .returns
            .iter()
            .map(|(line, values)| (*line, unstable(values)))
            .collect(),
        ..Default::default()
    }
}

/// Merges the values of the same point in several occurrences of a file.
fn merge_values<'v>(values: impl Iterator<Item = &'v IfValues> + Clone) -> IfValues {
    let names: HashSet<&Vec<u8>> = values.clone().flat_map(|v| v.keys()).collect();

    names
        .into_iter()
        .map(|name| {
            let mut found = values.clone().map(|v| v.get(name));
            let first = found.next().flatten();
            let value = match first {
                Some(value) if found.all(|other| other == Some(value)) => *value,
                _ => IfValue::Unknown,
            };
            (name.clone(), value)
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
enum EventKind {
    Before,
    After,
    Set,
    Document,
}

/// A line of the trace, see the module documentation.
#[derive(Debug)]
struct Event<'t> {
    kind: EventKind,
    line: usize,
    /// Group level of `set` and `document` events.
    level: usize,
    values: IfValues,
    path: &'t str,
    file: &'t str,
}

impl<'t> Event<'t> {
    fn parse(line: &'t str) -> Option<Self> {
        let (kind, rest) = line.split_once(' ')?;
        let kind = match kind {
            "before" => EventKind::Before,
            "after" => EventKind::After,
            "set" => EventKind::Set,
            "document" => EventKind::Document,
            _ => return None,
        };

        // Names and values do not contain a colon, the path may (e.g. on Windows)
        let (head, location) = rest.split_once(':')?;
        let (path, file) = location.rsplit_once(':')?;

        let mut head = head.split_ascii_whitespace();
        let line = head.next()?.parse().ok()?;
        let level = match kind {
            EventKind::Set | EventKind::Document => head.next()?.parse().ok()?,
            EventKind::Before | EventKind::After => 0,
        };
        let values: IfValues = head
            .map(|value| match value.split_once('=')? {
                (name, "1") => Some((name.as_bytes().to_vec(), IfValue::Known(true))),
                (name, "0") => Some((name.as_bytes().to_vec(), IfValue::Known(false))),
                _ => None,
            })
            .collect::<Option<_>>()?;
        if kind == EventKind::Set && values.len() != 1 {
            return None;
        }

        Some(Self {
            kind,
            line,
            level,
            values,
            path,
            file,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(values: &[(&str, IfValue)]) -> IfValues {
        values
            .iter()
            .map(|(name, value)| (name.as_bytes().to_vec(), *value))
            .collect()
    }

    use IfValue::*;

    /// A project in a temporary directory with the given (empty) files.
    fn project(files: &[&str]) -> temp_dir::TempDir {
        let dir = temp_dir::TempDir::new().unwrap();
        for file in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        dir
    }

    #[test]
    fn parses_events() {
        let event = Event::parse("after 3 anonymous=1 draft=0 :config:settings.tex").unwrap();

        assert_eq!(event.kind, EventKind::After);
        assert_eq!(event.line, 3);
        assert_eq!(
            event.values,
            values(&[("anonymous", Known(true)), ("draft", Known(false))])
        );
        assert_eq!(event.path, "config");
        assert_eq!(event.file, "settings.tex");

        let event = Event::parse("before 12 :C:/project:main.tex").unwrap();
        assert_eq!(event.path, "C:/project");
        assert!(event.values.is_empty());

        assert!(Event::parse("garbage").is_none());
        assert!(Event::parse("before x :a:b").is_none());
    }

    #[test]
    fn parses_set_and_document_events() {
        let event = Event::parse("set 30 0 final=1 ::").unwrap();
        assert_eq!(event.kind, EventKind::Set);
        assert_eq!((event.line, event.level), (30, 0));
        assert_eq!(event.values, values(&[("final", Known(true))]));
        assert_eq!(event.file, "");

        let event = Event::parse("document 38 1 final=1 draft=0 ::").unwrap();
        assert_eq!(event.kind, EventKind::Document);
        assert_eq!((event.line, event.level), (38, 1));
        assert_eq!(event.values.len(), 2);

        // A set event names exactly one conditional
        assert!(Event::parse("set 3 0 :a:b").is_none());
        assert!(Event::parse("set 3 0 a=1 b=0 :a:b").is_none());
    }

    #[test]
    fn values_set_by_commands_apply_on_the_next_lines() {
        let dir = project(&["main.tex", "template.sty"]);
        let root = dir.path().canonicalize().unwrap();
        let mut trace = IfTrace::new([b"final".to_vec()]);
        trace.add_main(
            &root.join("main.tex"),
            "set 1 0 final=0 ::alc-ng-if-probe.tex
after 1 ::alc-ng-if-probe.tex
before 2 ::template.sty
set 2 0 final=0 ::template.sty
after 2 final=0 ::template.sty
set 30 0 final=1 ::
set 31 2 final=0 ::
document 38 0 final=1 ::
",
        );

        let main = trace.state(&root.join("main.tex")).unwrap();
        assert_eq!(
            main.returns,
            vec![
                (2, values(&[("final", Known(false))])),
                (30, values(&[("final", Known(true))])),
                // Undone at the end of the group
                (31, values(&[("final", Unknown)])),
            ]
        );
    }

    /// The trace of a main file whose definitions use `\ifdraft`, with `draft` set at the
    /// given lines (`(line, value, group level)`) and `\begin{document}` on line 20.
    fn definition_state(sets: &[(usize, u8, usize)]) -> FileIfState {
        let dir = project(&["main.tex"]);
        let root = dir.path().canonicalize().unwrap();
        let mut trace = IfTrace::new([b"draft".to_vec()]);
        let mut events = String::new();
        for (line, value, level) in sets.iter().filter(|(line, ..)| *line < 20) {
            events += &format!("set {line} {level} draft={value} ::\n");
        }
        events += "document 20 0 ::\n";
        for (line, value, level) in sets.iter().filter(|(line, ..)| *line > 20) {
            events += &format!("set {line} {level} draft={value} ::\n");
        }
        trace.add_main(&root.join("main.tex"), &events);
        trace.state(&root.join("main.tex")).unwrap()
    }

    #[test]
    fn values_in_definitions() {
        // Not changed after the definition on line 5
        let state = definition_state(&[(3, 0, 0), (4, 1, 0)]);
        assert_eq!(
            state.value_in_definition(b"draft", 5, true),
            Some(Known(true))
        );
        // Changed on the line of the definition: it may happen before or after it
        assert_eq!(state.value_in_definition(b"draft", 4, true), Some(Unknown));

        // Changed in the preamble after the definition: the value in the document counts,
        // unless the command is used in the preamble
        let state = definition_state(&[(3, 0, 0), (10, 1, 0)]);
        assert_eq!(
            state.value_in_definition(b"draft", 5, false),
            Some(Known(true))
        );
        assert_eq!(state.value_in_definition(b"draft", 5, true), Some(Unknown));

        // Changed in the document
        let state = definition_state(&[(3, 0, 0), (25, 1, 0)]);
        assert_eq!(state.value_in_definition(b"draft", 5, false), Some(Unknown));
        assert_eq!(
            state.value_in_definition(b"draft", 22, false),
            Some(Unknown)
        );
        // A definition after the last change
        assert_eq!(
            state.value_in_definition(b"draft", 26, false),
            Some(Known(true))
        );

        // Set in a group, which undoes it untraced
        let state = definition_state(&[(3, 0, 0), (25, 0, 1)]);
        assert_eq!(state.value_in_definition(b"draft", 5, false), Some(Unknown));

        assert_eq!(state.value_in_definition(b"other", 5, false), None);
    }

    #[test]
    fn commands_used_in_the_preamble() {
        let dir = project(&["main.tex"]);
        let root = dir.path().canonicalize().unwrap();
        let main = root.join("main.tex");
        let mut trace = IfTrace::new([b"draft".to_vec()]);
        trace.add_main(&main, "set 2 0 draft=0 ::\ndocument 6 0 draft=0 ::\n");
        trace.add_preamble_commands(
            &main,
            br"\documentclass{article}
\newif\ifdraft
\newcommand{\note}{\ifdraft x\fi}
\def\other{\ifdraft y\fi}\edef\expanded{\other}
% \commented
\begin{document}\early
\note \let\ifdraft\iftrue
",
        );

        let state = trace.state(&main).unwrap();
        assert!(!state.is_used_in_preamble(br"\note"));
        assert!(!state.is_used_in_preamble(br"\commented"));
        assert!(state.is_used_in_preamble(br"\other"));
        // On the line of `\begin{document}`
        assert!(state.is_used_in_preamble(br"\early"));

        // Assigned by `\let`, which the probe does not see
        assert_eq!(state.value_in_definition(b"draft", 4, false), Some(Unknown));
    }

    #[test]
    fn traces_values_at_entry_and_after_returns() {
        let dir = project(&["main.tex", "config/settings.tex", "sections/intro.tex"]);
        let root = dir.path().canonicalize().unwrap();
        let mut trace = IfTrace::new([b"anonymous".to_vec()]);
        trace.add_main(
            &root.join("main.tex"),
            "after 1 ::alc-ng-if-probe.tex
before 3 ::article.cls
after 3 ::article.cls
before 4 :config:settings.tex
after 4 anonymous=1 :config:settings.tex
before 9 anonymous=0 :sections:intro.tex
after 9 anonymous=0 :sections:intro.tex
",
        );

        let main = trace.state(&root.join("main.tex")).unwrap();
        assert!(main.entry.is_empty());
        assert_eq!(
            main.returns,
            vec![
                (3, values(&[])),
                (4, values(&[("anonymous", Known(true))])),
                (9, values(&[("anonymous", Known(false))])),
            ]
        );

        let intro = trace.state(&root.join("sections/intro.tex")).unwrap();
        assert_eq!(intro.entry, values(&[("anonymous", Known(false))]));
        assert!(intro.returns.is_empty());
    }

    #[test]
    fn values_of_files_loaded_several_times_are_merged() {
        let dir = project(&["main.tex", "part.tex", "nested.tex"]);
        let root = dir.path().canonicalize().unwrap();
        let mut trace = IfTrace::new([b"a".to_vec(), b"b".to_vec()]);
        trace.add_main(
            &root.join("main.tex"),
            "before 2 a=1 b=1 ::part.tex
before 1 a=1 b=1 ::nested.tex
after 1 a=1 b=0 ::nested.tex
after 2 a=1 b=0 ::part.tex
before 4 a=0 b=0 ::part.tex
before 1 a=0 b=0 ::nested.tex
after 1 a=0 b=0 ::nested.tex
after 4 a=0 b=0 ::part.tex
",
        );

        let part = trace.state(&root.join("part.tex")).unwrap();
        assert_eq!(part.entry, values(&[("a", Unknown), ("b", Unknown)]));
        assert_eq!(
            part.returns,
            vec![(1, values(&[("a", Unknown), ("b", Known(false))]))]
        );

        // The nested file is loaded twice as well
        let nested = trace.state(&root.join("nested.tex")).unwrap();
        assert_eq!(nested.entry, values(&[("a", Unknown), ("b", Unknown)]));
    }

    #[test]
    fn untraced_files_have_unknown_values() {
        let dir = project(&["main.tex", "other.tex"]);
        let root = dir.path().canonicalize().unwrap();
        let mut trace = IfTrace::new([b"draft".to_vec()]);
        trace.add_main(&root.join("main.tex"), "");

        assert_eq!(
            trace.state(&root.join("other.tex")).unwrap().entry,
            values(&[("draft", Unknown)])
        );

        trace.mark_incomplete();
        assert!(trace.state(&root.join("other.tex")).is_none());
    }

    #[test]
    fn only_traceable_names_are_probed() {
        let trace = IfTrace::new([
            b"draft".to_vec(),
            b"my@flag".to_vec(),
            b"x1".to_vec(),
            b"draft".to_vec(),
        ]);

        assert_eq!(trace.names(), [b"draft".to_vec(), b"my@flag".to_vec()]);
        assert!(
            trace
                .probe()
                .contains(r"\alcng@value{draft}\alcng@value{my@flag}")
        );
    }

    #[test]
    fn probe_keeps_line_numbers() {
        assert_eq!(
            inject_probe(b"\\documentclass{article}\n"),
            b"\\input{alc-ng-if-probe}\\documentclass{article}\n"
        );
        assert_eq!(
            inject_probe(b"%&pdflatex\n\\documentclass{article}\n"),
            b"%&pdflatex\n\\input{alc-ng-if-probe}\\documentclass{article}\n"
        );
        assert_eq!(
            inject_probe(b"\xEF\xBB\xBF\\documentclass{article}"),
            b"\xEF\xBB\xBF\\input{alc-ng-if-probe}\\documentclass{article}"
        );
    }
}
