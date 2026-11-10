//! Values of custom conditionals (`\newif`) across files, traced while compiling.
//!
//! The cleaner evaluates custom conditionals while it walks a file, but each file is cleaned on
//! its own. A conditional is often defined and set in one file and used in another one, which is
//! loaded with `\input` or `\include`, possibly several times with different values.
//! Thus, the cleaner needs the values at the start of each file and after each file loaded from
//! it returned.
//!
//! TeX knows these values. Before compiling, a probe is loaded at the start of each main file.
//! It uses the `file/before` and `file/after` hooks of LaTeX (2020-10 and later) to write the
//! values of all conditionals of the project into `<jobname>.alcng-ifs` whenever a file is
//! loaded and when it ends:
//!
//! ```text
//! before 3 :config:settings.tex
//! after 3 anonymous=1 draft=0 :config:settings.tex
//! ```
//!
//! Each line holds the event, the line of the loading file (`\inputlineno`), the values of the
//! defined conditionals, and the path and name of the loaded file (`\CurrentFilePath`,
//! `\CurrentFile`). Files loaded with the primitive syntax (`\input file`) do not trigger the
//! hooks; the values in such files are unknown.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
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
    /// Values after a file loaded from this file returned, with the (1-based) line of this
    /// file where it was loaded. Ordered as in the file.
    pub returns: Vec<(usize, IfValues)>,
}

/// The values of the conditionals in all files of the project, see the module documentation.
#[derive(Debug, Default)]
pub struct IfTrace {
    /// All traced conditionals.
    names: Vec<Vec<u8>>,
    /// Each time a file is loaded, by the canonical path of the file.
    files: HashMap<PathBuf, Vec<FileIfState>>,
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
            files: HashMap::new(),
            complete: true,
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
        let values: String = self
            .names
            .iter()
            .map(|name| format!("\\alcng@value{{{}}}", String::from_utf8_lossy(name)))
            .collect();

        // The probe stops without file hooks (LaTeX older than 2020-10) and if it is loaded
        // again, e.g. as a main file is loaded by another one.
        // `\ifx` compares the meaning of a conditional with `\iftrue` and `\iffalse` without
        // evaluating it. The comparisons are split into macros, as `\iftrue` and `\iffalse` in
        // skipped branches would count as nested conditionals.
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
\def\alcng@event#1{{\def\alcng@values{{}}{values}\immediate\write\alcng@out{{#1 \the\inputlineno\alcng@values\space:\CurrentFilePath:\CurrentFile}}}}
\AddToHook{{file/before}}{{\alcng@event{{before}}}}
\AddToHook{{file/after}}{{\alcng@event{{after}}}}
\makeatother
"
        )
    }

    /// Reads the trace written by the probe for the main file at `main` (a canonical path).
    pub fn add_main(&mut self, main: &Path, trace: &str) {
        let base = main.parent().unwrap_or(main);

        // The occurrences of the files and the stack of currently loaded files.
        let mut occurrences = vec![(Some(main.to_path_buf()), FileIfState::default())];
        let mut stack = vec![(0, String::new())];

        for line in trace.lines() {
            let Some(event) = Event::parse(line) else {
                debug!("Ignoring malformed line of the conditional trace: {line:?}");
                continue;
            };

            match event.kind {
                EventKind::Before => {
                    let path = base.join(event.path).join(event.file);
                    let path = path.canonicalize().ok().filter(|p| p.is_file());

                    occurrences.push((
                        path,
                        FileIfState {
                            entry: event.values,
                            returns: Vec::new(),
                        },
                    ));
                    stack.push((occurrences.len() - 1, event.file.to_owned()));
                }
                EventKind::After => {
                    // Only the root (main file) remains or the event does not belong to the
                    // file on top, e.g. the probe itself, which is loaded before the hooks are set
                    match stack.last() {
                        Some((_, file)) if stack.len() > 1 && file == event.file => {
                            stack.pop();
                        }
                        _ => continue,
                    }

                    let (parent, _) = stack.last().expect("the main file is never removed");
                    occurrences[*parent]
                        .1
                        .returns
                        .push((event.line, event.values));
                }
            }
        }

        for (path, state) in occurrences {
            if let Some(path) = path {
                trace!(
                    "Traced conditionals of {}: {:?}",
                    path.display(),
                    state.entry
                );
                self.files.entry(path).or_default().push(state);
            }
        }
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
            Some(occurrences) => Some(merge(occurrences)),
            None => Some(FileIfState {
                entry: self
                    .names
                    .iter()
                    .map(|name| (name.clone(), IfValue::Unknown))
                    .collect(),
                returns: Vec::new(),
            }),
        }
    }
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
        return FileIfState { entry, returns };
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
}

/// A line of the trace, see the module documentation.
#[derive(Debug)]
struct Event<'t> {
    kind: EventKind,
    line: usize,
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
            _ => return None,
        };

        // Names and values do not contain a colon, the path may (e.g. on Windows)
        let (head, location) = rest.split_once(':')?;
        let (path, file) = location.rsplit_once(':')?;

        let mut head = head.split_ascii_whitespace();
        let line = head.next()?.parse().ok()?;
        let values = head
            .map(|value| match value.split_once('=')? {
                (name, "1") => Some((name.as_bytes().to_vec(), IfValue::Known(true))),
                (name, "0") => Some((name.as_bytes().to_vec(), IfValue::Known(false))),
                _ => None,
            })
            .collect::<Option<_>>()?;

        Some(Self {
            kind,
            line,
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
