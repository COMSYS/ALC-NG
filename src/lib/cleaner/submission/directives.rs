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
/// whole word, in any case (e.g. `% !alc-ng`). Any remaining whitespace‑separated
/// tokens are the keywords.
pub fn directive_keywords(text: &[u8]) -> Option<Vec<&str>> {
    let line = std::str::from_utf8(text).ok()?;
    let rest = line.trim_start().strip_prefix('%')?.trim_start();
    let tag = rest.get(..DIRECTIVE_TAG.len())?;
    if !tag.eq_ignore_ascii_case(DIRECTIVE_TAG) {
        return None;
    }
    let rest = &rest[DIRECTIVE_TAG.len()..];

    // The tag must be a whole word: followed by whitespace or end of line.
    // This rejects look‑alikes such as `% !ALC-NGX`.
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    Some(rest.split_whitespace().collect())
}

/// Tags of the magic comments that the cleaner keeps: its own [`DIRECTIVE_TAG`] and
/// the tags that editors and build tools read from a `.tex` file, e.g.
/// `% !TEX root = main.tex` (TeXShop, TeXstudio, LaTeX Workshop) or
/// `% !BIB program = biber` (TeXstudio). Matched case-insensitively.
const MAGIC_COMMENT_TAGS: [&str; 3] = [DIRECTIVE_TAG, "!TEX", "!BIB"];

/// Returns `true` if the comment is a magic comment that the cleaner keeps, such as
/// `% !ALC-NG keep-ifs` or `% !TEX root = main.tex`.
///
/// Like a directive, the tag must directly follow the `%` (and optional
/// whitespace) and be a whole word.
pub fn is_magic_comment(text: &[u8]) -> bool {
    let Some(rest) = text.trim_ascii_start().strip_prefix(b"%") else {
        return false;
    };
    let rest = rest.trim_ascii_start();
    MAGIC_COMMENT_TAGS.iter().any(|tag| {
        rest.len() >= tag.len()
            && rest[..tag.len()].eq_ignore_ascii_case(tag.as_bytes())
            && rest.get(tag.len()).is_none_or(|c| c.is_ascii_whitespace())
    })
}

/// Parses all ALC‑NG directives from a file's syntax tree.
///
/// Only real `line_comment` nodes are inspected, so directive‑looking text
/// inside verbatim/lstlisting environments (which parse as `verbatim` /
/// `source_code` nodes) is ignored.
///
/// Directives that start at or after `until` are ignored. The cleaner passes the end
/// of the document environment, since it removes the content after it, including any
/// directive there.
pub fn parse_directives(root: Node, content: &[u8], until: Option<usize>) -> FileDirectives {
    let mut directives = FileDirectives::default();
    collect_directives(root, content, until, &mut directives);
    directives
}

fn collect_directives(
    node: Node,
    content: &[u8],
    until: Option<usize>,
    directives: &mut FileDirectives,
) {
    if until.is_some_and(|until| node.start_byte() >= until) {
        return;
    }
    if node.grammar_name() == "line_comment"
        && let Some(keywords) = directive_keywords(&content[node.byte_range()])
    {
        for keyword in keywords {
            apply_keyword(keyword, directives);
        }
    }

    let mut i: u32 = 0;
    while let Some(child) = node.child(i) {
        collect_directives(child, content, until, directives);
        i += 1;
    }
}

/// Applies a directive keyword, ignoring its case.
fn apply_keyword(keyword: &str, directives: &mut FileDirectives) {
    match keyword.to_ascii_lowercase().as_str() {
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
        parse_directives(tree.root_node(), input.as_bytes(), None)
    }

    #[test]
    fn magic_comment_matching() {
        assert!(is_magic_comment(b"% !TEX root = main.tex"));
        assert!(is_magic_comment(b"%!TEX program = xelatex"));
        assert!(is_magic_comment(b"% !TeX spellcheck = en_US"));
        assert!(is_magic_comment(b"% !BIB program = biber"));
        assert!(is_magic_comment(b"%  !ALC-NG keep-ifs"));
        assert!(is_magic_comment(b"% !TEX"));
        assert!(is_magic_comment(b"%!ALC-NG"));
        assert!(!is_magic_comment(b"% !ALC-NGX keep-all"));
        // Not magic comments
        assert!(!is_magic_comment(b"% a normal comment"));
        assert!(!is_magic_comment(b"% see !TEX root"));
        assert!(!is_magic_comment(b"% !TEXT root = main.tex"));
        assert!(!is_magic_comment(b"%% !TEX root = main.tex"));
        assert!(!is_magic_comment(b"\\% !TEX root = main.tex"));
    }

    #[test]
    fn directives_after_until_are_ignored() {
        let input =
            "\\documentclass{article}\n\\begin{document}\n\\end{document}\n% !ALC-NG keep-ifs\n";
        let tree = parse(input.as_bytes()).unwrap();
        let end = input.find("\n% !ALC").unwrap();
        assert!(parse_directives(tree.root_node(), input.as_bytes(), Some(end)).is_empty());
        assert!(parse_directives(tree.root_node(), input.as_bytes(), None).keep_ifs);
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
        // The tag ignores case
        assert_eq!(
            directive_keywords(b"% !alc-ng keep-ifs"),
            Some(vec!["keep-ifs"])
        );
        assert_eq!(directive_keywords(b"%!Alc-Ng"), Some(vec![]));
        assert_eq!(directive_keywords(b"% !alc-ngx keep-all"), None);
        // Non-ASCII after `%` does not panic
        assert_eq!(directive_keywords(b"% !\xc3\xa4"), None);
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
    fn directive_tag_and_keywords_ignore_case() {
        let d = directives_for("% !alc-ng Keep-Comments KEEP-IFS\n\\documentclass{article}\n");
        assert!(d.keep_comments);
        assert!(d.keep_ifs);
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
