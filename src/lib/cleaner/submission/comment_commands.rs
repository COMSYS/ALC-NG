//! Submission-wide classification of custom comment commands.
//!
//! A comment command is a command with an empty body, e.g. `\newcommand{\note}[1]{}`, whose
//! invocations can be removed together with their arguments without changing the PDF.
//!
//! An empty body alone is no proof of that. Empty definitions also reset hooks
//! (`\renewcommand{\thefootnote}{}`), initialize state that is filled later
//! (`\g@addto@macro`), act as flags (`\@ifundefined`) or typeset what looks like their argument
//! (`\newcommand{\mt}{}` used as `\mt{text}`). Thus, definitions are always kept and only the
//! invocations of commands that pass all checks of [`CommentCommands::analyze`] are removed.
//! A command that fails a check is left untouched, which always produces the original PDF.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{debug, info, trace};
use tree_sitter::Node;

use crate::cleaner::{config::CleanerConfig, submission::parsed_file::ContentStripper};

/// Upper bound of analysis rounds. Each round may find commands whose body only consists of
/// invocations of comment commands found in the previous round.
const MAX_ROUNDS: usize = 4;

/// Bytes after a command that refers to other commands by name (e.g. `\csname`) that are searched for names.
const DYNAMIC_REFERENCE_WINDOW: usize = 256;

/// Commands that refer to other commands by their name, e.g. `\csname note\endcsname`.
const DYNAMIC_REFERENCE_COMMANDS: [&[u8]; 41] = [
    br"\csname",
    br"\ifcsname",
    br"\@namedef",
    br"\@nameuse",
    br"\@ifundefined",
    br"\@ifdefinable",
    br"\csdef",
    br"\csgdef",
    br"\csedef",
    br"\csxdef",
    br"\csuse",
    br"\cslet",
    br"\csletcs",
    br"\letcs",
    br"\csundef",
    br"\csgundef",
    br"\ifcsdef",
    br"\ifcsundef",
    br"\ifcsmacro",
    br"\ifcsempty",
    br"\ifcsvoid",
    br"\ifcsequal",
    br"\ifcsstring",
    br"\ifcsprotected",
    br"\ifcsparam",
    br"\ifcsprefix",
    br"\ifcsltxprotect",
    br"\csappto",
    br"\csgappto",
    br"\cseappto",
    br"\csxappto",
    br"\cspreto",
    br"\csgpreto",
    br"\csepreto",
    br"\csxpreto",
    br"\csshow",
    br"\csmeaning",
    br"\ifcsstrequal",
    br"\csshowcmd",
    br"\ShowCommand",
    br"\NewCommandCopy",
];

/// Commands that take the directly following command as a token instead of invoking it,
/// e.g. `\g@addto@macro\hook{...}` or `\ifx\note\empty`.
const MANIPULATING_COMMANDS: [&[u8]; 54] = [
    br"\let",
    br"\glet",
    br"\global",
    br"\expandafter",
    br"\noexpand",
    br"\unexpanded",
    br"\protected",
    br"\long",
    br"\outer",
    br"\futurelet",
    br"\afterassignment",
    br"\aftergroup",
    br"\show",
    br"\showthe",
    br"\meaning",
    br"\string",
    br"\ifx",
    br"\ifdefined",
    br"\g@addto@macro",
    br"\addto",
    br"\appto",
    br"\gappto",
    br"\eappto",
    br"\xappto",
    br"\preto",
    br"\gpreto",
    br"\epreto",
    br"\xpreto",
    br"\apptocmd",
    br"\pretocmd",
    br"\patchcmd",
    br"\xapptocmd",
    br"\xpretocmd",
    br"\xpatchcmd",
    br"\robustify",
    br"\newrobustcmd",
    br"\renewrobustcmd",
    br"\providerobustcmd",
    br"\MakeRobust",
    br"\CheckCommand",
    br"\undef",
    br"\gundef",
    br"\ifdef",
    br"\ifundef",
    br"\ifdefmacro",
    br"\ifdefempty",
    br"\ifdefvoid",
    br"\ifdefparam",
    br"\ifdefprefix",
    br"\ifdefprotected",
    br"\ifdefltxprotect",
    br"\ifdefequal",
    br"\ifdefstring",
    br"\ShowCommand",
];

/// Commands that compare the second of the two following tokens, e.g. `\ifx\empty\note`.
const TWO_TOKEN_COMMANDS: [&[u8]; 4] = [br"\ifx", br"\let", br"\glet", br"\futurelet"];

/// A command whose invocations are removed together with their arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentCommand {
    /// Number of mandatory arguments, at least one.
    pub arity: usize,
    /// The command takes an optional argument with a default value.
    pub optional: bool,
    /// Name of the file that contains the definition.
    pub filename: String,
    /// Byte after the end of the definition.
    pub end_byte: usize,
    /// The definition is part of the document body, thus invocations in other files may precede it.
    pub in_document: bool,
}

/// A definition with an empty body that is eligible as comment command, found while stripping a file.
#[derive(Debug, Clone)]
pub struct EmptyDefinition {
    /// Name of the defined command, including the backslash.
    pub name: Vec<u8>,
    pub command: CommentCommand,
}

/// A parsed file that is part of the analysis.
pub struct AnalyzedFile<'a> {
    pub content: &'a [u8],
    pub root: Node<'a>,
    /// The name the file is cleaned with.
    pub filename: &'a str,
}

/// The comment commands of a submission, see the module documentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentCommands {
    commands: HashMap<Vec<u8>, CommentCommand>,
}

impl CommentCommands {
    /// Determines the comment commands of all given files.
    ///
    /// A command qualifies if its single definition has an empty body after cleaning,
    /// takes at least one mandatory argument and holds for the rest of the document,
    /// and if it is never referenced other than by a plain invocation.
    pub fn analyze(files: &[AnalyzedFile], cleaner_config: Arc<CleanerConfig>) -> Arc<Self> {
        let mut references = References::default();
        for file in files {
            references.collect(file.root, file.content);
        }

        let mut commands = Arc::new(Self::default());
        let mut rejected = Vec::new();

        for _ in 0..MAX_ROUNDS {
            // Bodies are cleaned with the comment commands of the previous round,
            // thus a body that only invokes comment commands is empty as well.
            let mut definitions: HashMap<Vec<u8>, Vec<CommentCommand>> = HashMap::new();
            for file in files {
                match ContentStripper::empty_definitions(
                    file.content,
                    file.root,
                    file.filename,
                    cleaner_config.clone(),
                    commands.clone(),
                ) {
                    Ok(found) => {
                        for definition in found {
                            definitions
                                .entry(definition.name)
                                .or_default()
                                .push(definition.command);
                        }
                    }
                    // Cleaning the file reports the error.
                    Err(err) => trace!(
                        "Skipping comment command analysis of {}: {err}",
                        file.filename
                    ),
                }
            }

            let mut next = Self::default();
            rejected.clear();
            for (name, mut found) in definitions {
                match references.rejection(&name) {
                    None if found.len() == 1 => {
                        next.commands.insert(name, found.pop().unwrap());
                    }
                    None => rejected.push((name, "it is defined more than once")),
                    Some(reason) => rejected.push((name, reason)),
                }
            }

            let done = next == *commands;
            commands = Arc::new(next);
            if done {
                break;
            }
        }

        for (name, reason) in &rejected {
            debug!(
                "Keeping invocations of empty command {}, as {reason}",
                String::from_utf8_lossy(name)
            );
        }
        if !commands.commands.is_empty() {
            let mut names: Vec<_> = commands
                .commands
                .keys()
                .map(|n| String::from_utf8_lossy(n))
                .collect();
            names.sort();
            info!(
                "Removing invocations of comment commands: {}",
                names.join(", ")
            );
        }

        commands
    }

    /// Returns the comment command with the given name.
    pub fn get(&self, name: &[u8]) -> Option<&CommentCommand> {
        self.commands.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// How the commands of a submission are referenced, independent of their definitions' bodies.
#[derive(Default)]
struct References {
    /// Number of definitions per command, in any form (`\newcommand`, `\def`, `\let`, …).
    definitions: HashMap<Vec<u8>, usize>,
    /// Commands referenced other than by a plain invocation, e.g. `\let\a\note` or `\ifx\note\empty`.
    unsafe_references: HashSet<Vec<u8>>,
    /// Bare names (without backslash) referenced by name, e.g. by `\csname note\endcsname`.
    dynamic_references: HashSet<Vec<u8>>,
}

impl References {
    /// Collects the references of all nodes in the subtree.
    fn collect(&mut self, node: Node, content: &[u8]) {
        let declaration = node.child_by_field_name("declaration");
        if let Some(name) = declaration.and_then(|d| declared_name(d, content)) {
            *self.definitions.entry(name.to_owned()).or_default() += 1;
        }

        if matches!(node.kind(), "command_name" | "todo_command_name") {
            self.collect_reference(node, content);
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if Some(child) != declaration {
                self.collect(child, content);
            }
        }
    }

    fn collect_reference(&mut self, node: Node, content: &[u8]) {
        let name = &content[node.byte_range()];

        if DYNAMIC_REFERENCE_COMMANDS.contains(&name) {
            self.collect_dynamic_references(name, &content[node.end_byte()..]);
        }

        let invocation = node
            .parent()
            .filter(|p| matches!(p.kind(), "generic_command" | "todo"))
            .filter(|p| p.child_by_field_name("command") == Some(node));

        let safe = match invocation {
            // `\robustify{\note}` or `\newcommand{\a}{\note}` pass the command on as a token
            Some(invocation) => {
                !is_manipulated(content, node.start_byte())
                    && !(invocation.child_by_field_name("arg").is_none()
                        && next_byte(content, invocation.end_byte()) == Some(b'}'))
            }
            None => false,
        };

        if !safe {
            self.unsafe_references.insert(name.to_owned());
        }
    }

    /// Collects the names that follow a command referring to commands by name, e.g. `\csname`.
    fn collect_dynamic_references(&mut self, command: &[u8], rest: &[u8]) {
        let rest = &rest[..rest.len().min(DYNAMIC_REFERENCE_WINDOW)];
        let region = if command == br"\csname" || command == br"\ifcsname" {
            match find(rest, br"\endcsname") {
                Some(end) => &rest[..end],
                None => rest,
            }
        } else {
            leading_groups(rest, 2)
        };

        let mut i = 0;
        while i < region.len() {
            let start = i;
            while i < region.len() && is_letter(region[i]) {
                i += 1;
            }
            if i > start && (start == 0 || region[start - 1] != b'\\') {
                self.dynamic_references.insert(region[start..i].to_owned());
            }
            i += 1;
        }
    }

    /// Returns why the invocations of the command must not be removed, if they must not.
    fn rejection(&self, name: &[u8]) -> Option<&'static str> {
        if self.definitions.get(name).copied().unwrap_or_default() > 1 {
            Some("it is defined more than once")
        } else if self.unsafe_references.contains(name) {
            Some("it is referenced other than by an invocation")
        } else if self.dynamic_references.contains(&name[1..]) {
            Some("it is referenced by name")
        } else {
            None
        }
    }
}

/// Returns the name of the command declared by a `declaration` node, if any.
fn declared_name<'a>(declaration: Node, content: &'a [u8]) -> Option<&'a [u8]> {
    if declaration.kind() == "command_name" {
        return Some(&content[declaration.byte_range()]);
    }

    let mut cursor = declaration.walk();
    declaration
        .children(&mut cursor)
        .find(|c| c.kind() == "command_name")
        .map(|c| &content[c.byte_range()])
}

/// Returns `true` if a command preceding the given position takes the command at the position as a token.
fn is_manipulated(content: &[u8], start: usize) -> bool {
    let Some((previous_start, previous)) = previous_control_word(content, start) else {
        return false;
    };
    if MANIPULATING_COMMANDS.contains(&previous) {
        return true;
    }

    previous_control_word(content, previous_start)
        .is_some_and(|(_, before)| TWO_TOKEN_COMMANDS.contains(&before))
}

/// Returns the start and text of the control word directly in front of `end`,
/// skipping whitespace and an `=` (as in `\let\a=\b`).
fn previous_control_word(content: &[u8], end: usize) -> Option<(usize, &[u8])> {
    let mut stop = end;
    while stop > 0 && (content[stop - 1].is_ascii_whitespace() || content[stop - 1] == b'=') {
        stop -= 1;
    }

    let mut start = stop;
    while start > 0 && is_letter(content[start - 1]) {
        start -= 1;
    }

    (start < stop && start > 0 && content[start - 1] == b'\\')
        .then(|| (start - 1, &content[start - 1..stop]))
}

/// Returns the first byte from `start` on that is not whitespace.
fn next_byte(content: &[u8], start: usize) -> Option<u8> {
    content[start..]
        .iter()
        .find(|b| !b.is_ascii_whitespace())
        .copied()
}

/// Returns the prefix of `content` that ends with its `count`-th top-level brace group.
fn leading_groups(content: &[u8], count: usize) -> &[u8] {
    let mut depth = 0usize;
    let mut groups = 0;
    let mut i = 0;

    while i < content.len() {
        match content[i] {
            b'\\' => i += 1,
            b'{' => depth += 1,
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    groups += 1;
                    if groups == count {
                        return &content[..=i];
                    }
                }
            }
            _ => (),
        }
        i += 1;
    }

    content
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Letters of control words with `\makeatletter` in effect.
fn is_letter(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'@'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_control_word_skips_whitespace_and_equals() {
        let content = br"\let\a = \note";
        let (start, word) = previous_control_word(content, 9).unwrap();
        assert_eq!(word, br"\a");
        assert_eq!(previous_control_word(content, start).unwrap().1, br"\let");
        assert!(is_manipulated(content, 9));
    }

    #[test]
    fn plain_invocation_is_not_manipulated() {
        assert!(!is_manipulated(br"Text \note{x}", 5));
        assert!(!is_manipulated(br"\noindent\note{x}", 9));
        assert!(is_manipulated(br"\g@addto@macro\note{x}", 14));
    }

    #[test]
    fn leading_groups_stops_after_count_groups() {
        assert_eq!(leading_groups(br"{a}{b}{c}", 2), br"{a}{b}");
        assert_eq!(leading_groups(br"{a{b}}\x{c}rest", 2), br"{a{b}}\x{c}");
        assert_eq!(leading_groups(br"{\}}", 1), br"{\}}");
    }
}
