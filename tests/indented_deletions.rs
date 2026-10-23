#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    fn clean(input: &[u8]) -> String {
        let tree = parse(input).unwrap();
        let (stripped, deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(deletion_stats.grammar_errors.is_empty());
        String::from_utf8(stripped.unwrap()).unwrap()
    }

    #[test]
    fn indented_invocation_in_caption_is_replaced() {
        let input = br#"\newcommand{\added}[1]{}
\begin{figure}
  \caption{
  Old text
  \added{
  This is something new
  }
  Continued old text
  }
\end{figure}
"#;

        assert_eq!(
            clean(input),
            r#"\begin{figure}
  \caption{
  Old text
  {}
  Continued old text
  }
\end{figure}
"#
        );
    }

    #[test]
    fn invocation_with_trailing_spaces_is_replaced() {
        let input = b"\\newcommand{\\note}[1]{}\nOld text\n    \\note{gone}   \n    Continued\n";

        assert_eq!(clean(input), "Old text\n    {}   \n    Continued\n");
    }

    #[test]
    fn invocation_with_tab_indentation_is_replaced() {
        let input = b"\\newcommand{\\note}[1]{}\nOld text\n\t\\note{gone}\nContinued\n";

        assert_eq!(clean(input), "Old text\n\t{}\nContinued\n");
    }

    #[test]
    fn inline_invocation_is_replaced() {
        let input = b"\\newcommand{\\note}[1]{}\n  Old \\note{gone} text\n";

        assert_eq!(clean(input), "  Old {} text\n");
    }

    #[test]
    fn indented_empty_if_block() {
        let input = br#"\newif\ifdraft
\draftfalse
\caption{
    Old text
    \ifdraft
      draft only
    \fi
    Continued
}
"#;

        let cleaned = clean(input);
        assert!(cleaned.contains("    Old text\n    Continued\n"));
        assert!(!cleaned.contains("draft only"));
    }

    #[test]
    fn empty_if_block_keeps_paragraph_break() {
        let input = br#"\newif\ifdraft
\draftfalse
First paragraph
\ifdraft
draft only
\fi

Second paragraph
"#;

        let cleaned = clean(input);
        assert!(cleaned.contains("First paragraph\n\nSecond paragraph\n"));
    }

    #[test]
    fn invocation_in_math_is_deleted() {
        let input = b"\\newcommand{\\note}[1]{}\n$\\note{x}-b$ and \\[ a \\note{y} + b \\]\n";

        assert_eq!(clean(input), "$-b$ and \\[ a  + b \\]\n");
    }

    #[test]
    fn newcommand_without_argc_keeps_definition() {
        let input = b"\\newcommand{\\keep}{x}\n\\keep\n";

        assert_eq!(clean(input), "\\newcommand{\\keep}{x}\n\\keep\n");
    }

    #[test]
    fn newcommand_without_argc_detects_comment_command() {
        let input = b"\\newcommand{\\note}{}\nBefore \\note after\n";

        assert_eq!(clean(input), "Before {} after\n");
    }

    #[test]
    fn active_character_comment_definition_is_kept() {
        // The stripper cannot recognize active-character invocations, so it
        // must keep the definition instead of deleting it and leaving the
        // invocation behind.
        let input = b"\\newcommand{\"}[1]{}\nBefore \"{gone} after\n";

        assert_eq!(
            clean(input),
            "\\newcommand{\"}[1]{}\nBefore \"{gone} after\n"
        );
    }

    #[test]
    fn invocation_in_text_inside_math_is_replaced() {
        let input = b"\\newcommand{\\note}[1]{}\n$a \\text{if \\note{x} b}$\n";

        assert_eq!(clean(input), "$a \\text{if {} b}$\n");
    }
}
