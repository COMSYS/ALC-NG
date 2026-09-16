use log::warn;
use tree_sitter::Node;

/// Magic tag that marks a line comment as an ALC-NG directive.
pub const DIRECTIVE_TAG: &str = "!ALC-NG";

/// Per-file cleaning directives parsed from `% !ALC-NG …` line comments.
///
/// Each flag disables one aspect of the cleaning for the file the
/// directive was found in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDirectives {
    /// Keep all comments (line, suffix, `comment` environment, custom
    /// comment commands and their invocations).
    pub keep_comments: bool,
    /// Keep conditional (`\if…`) blocks unevaluated, preserving both branches.
    pub keep_ifs: bool,
    /// Keep content that appears after `\end{document}`.
    pub keep_tail: bool,
    /// Do not clean the file at all; keep it exactly as is.
    pub keep_all: bool,
}

impl FileDirectives {
    /// Returns `true` if no directive was parsed.
    pub fn is_empty(&self) -> bool {
        !self.keep_comments && !self.keep_ifs && !self.keep_tail && !self.keep_all
    }

    /// Returns a human-readable summary of the active directives, e.g.
    /// `"keep-comments, keep-ifs, keep-tail"`.
    ///
    /// `keep-all` supersedes the rest: if it is set, only `"keep-all"` is
    /// returned — unless all other flags are set as well, in which case
    /// they are listed first, followed by `"keep-all"`.
    pub fn describe(&self) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(3);
        if self.keep_comments {
            parts.push("keep-comments");
        }
        if self.keep_ifs {
            parts.push("keep-ifs");
        }
        if self.keep_tail {
            parts.push("keep-tail");
        }
        match (self.keep_all, parts.len()) {
            (true, _) | (_, 3) => "keep-all".to_string(),
            _ => parts.join(", "),
        }
    }
}

/// Returns the keywords of a `% !ALC-NG …` directive line, or `None` if the
/// given comment text is not a directive.
///
/// The comment must start with `%`, followed by the [`DIRECTIVE_TAG`] as a
/// whole word. Any remaining whitespace‑separated tokens are the keywords.
pub fn directive_keywords(text: &[u8]) -> Option<Vec<&str>> {
    let line = std::str::from_utf8(text).ok()?;
    let rest = line.trim_start().strip_prefix('%')?.trim_start();
    let rest = rest.strip_prefix(DIRECTIVE_TAG)?;

    // The tag must be a whole word: followed by whitespace or end of line.
    // This rejects look‑alikes such as `% !ALC-NGX`.
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    Some(rest.split_whitespace().collect())
}

/// Parses all ALC‑NG directives from a file's syntax tree.
///
/// Only real `line_comment` nodes are inspected, so directive‑looking text
/// inside verbatim/lstlisting environments (which parse as `verbatim` /
/// `source_code` nodes) is ignored.
pub fn parse_directives(root: Node, content: &[u8]) -> FileDirectives {
    let mut directives = FileDirectives::default();
    collect_directives(root, content, &mut directives);
    directives
}

fn collect_directives(node: Node, content: &[u8], directives: &mut FileDirectives) {
    if node.grammar_name() == "line_comment"
        && let Some(keywords) = directive_keywords(&content[node.byte_range()])
    {
        for keyword in keywords {
            apply_keyword(keyword, directives);
        }
    }

    let mut i: u32 = 0;
    while let Some(child) = node.child(i) {
        collect_directives(child, content, directives);
        i += 1;
    }
}

fn apply_keyword(keyword: &str, directives: &mut FileDirectives) {
    match keyword {
        "keep-comments" => directives.keep_comments = true,
        "keep-ifs" | "keep-conditionals" => directives.keep_ifs = true,
        "keep-tail" | "keep-oob" => directives.keep_tail = true,
        "keep-all" | "skip" | "noclean" => directives.keep_all = true,
        other => warn!("Ignoring unknown ALC-NG directive '{}'", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::parse;

    fn directives_for(input: &str) -> FileDirectives {
        let tree = parse(input.as_bytes()).unwrap();
        parse_directives(tree.root_node(), input.as_bytes())
    }

    #[test]
    fn keyword_matching() {
        assert_eq!(
            directive_keywords(b"% !ALC-NG keep-comments"),
            Some(vec!["keep-comments"])
        );
        assert_eq!(
            directive_keywords(b"% !ALC-NG keep-ifs keep-tail"),
            Some(vec!["keep-ifs", "keep-tail"])
        );
        assert_eq!(directive_keywords(b"% !ALC-NG"), Some(vec![]));
        assert_eq!(
            directive_keywords(b"%!ALC-NG keep-all"),
            Some(vec!["keep-all"])
        );
        // Not a directive
        assert_eq!(directive_keywords(b"% a normal comment"), None);
        // Tag must be a whole word
        assert_eq!(directive_keywords(b"% !ALC-NGX keep-all"), None);
        // Not a comment at all
        assert_eq!(directive_keywords(b"\\% !ALC-NG keep-all"), None);
    }

    #[test]
    fn parses_directives_from_tree() {
        let input =
            "% header comment\n% !ALC-NG keep-comments keep-ifs\n\\documentclass{article}\n";
        let d = directives_for(input);
        assert!(d.keep_comments);
        assert!(d.keep_ifs);
        assert!(!d.keep_tail);
        assert!(!d.keep_all);
    }

    #[test]
    fn no_directive_when_absent() {
        let d = directives_for("% just a comment\n\\documentclass{article}\n");
        assert!(d.is_empty());
    }

    #[test]
    fn directive_in_verbatim_is_ignored() {
        let input = "\\begin{verbatim}\n% !ALC-NG keep-all\n\\end{verbatim}\n";
        let d = directives_for(input);
        assert!(d.is_empty());
    }

    #[test]
    fn describe_lists_active_directives() {
        assert_eq!(FileDirectives::default().describe(), "", "no directives");
        assert_eq!(
            FileDirectives {
                keep_comments: true,
                ..Default::default()
            }
            .describe(),
            "keep-comments"
        );
        assert_eq!(
            FileDirectives {
                keep_comments: true,
                keep_ifs: true,
                keep_tail: true,
                ..Default::default()
            }
            .describe(),
            "keep-all"
        );
    }

    #[test]
    fn describe_keep_all_supersedes() {
        let keep_all = FileDirectives {
            keep_all: true,
            ..Default::default()
        };
        assert_eq!(keep_all.describe(), "keep-all");

        let all = FileDirectives {
            keep_comments: true,
            keep_ifs: true,
            keep_tail: true,
            keep_all: true,
        };
        assert_eq!(all.describe(), "keep-all");

        let partial = FileDirectives {
            keep_comments: true,
            keep_tail: true,
            keep_all: true,
            ..Default::default()
        };
        assert_eq!(partial.describe(), "keep-all");
    }

    #[test]
    fn directive_in_listing_is_ignored() {
        let input = "\\begin{lstlisting}\n% !ALC-NG keep-all\n\\end{lstlisting}\n";
        let d = directives_for(input);
        assert!(d.is_empty());
    }
}
