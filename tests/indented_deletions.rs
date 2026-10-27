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
            r#"\newcommand{\added}[1]{}
\begin{figure}
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

        assert_eq!(
            clean(input),
            "\\newcommand{\\note}[1]{}\nOld text\n    {}   \n    Continued\n"
        );
    }

    #[test]
    fn invocation_with_tab_indentation_is_replaced() {
        let input = b"\\newcommand{\\note}[1]{}\nOld text\n\t\\note{gone}\nContinued\n";

        assert_eq!(
            clean(input),
            "\\newcommand{\\note}[1]{}\nOld text\n\t{}\nContinued\n"
        );
    }

    #[test]
    fn inline_invocation_is_replaced() {
        let input = b"\\newcommand{\\note}[1]{}\n  Old \\note{gone} text\n";

        assert_eq!(clean(input), "\\newcommand{\\note}[1]{}\n  Old {} text\n");
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

        assert_eq!(
            clean(input),
            "\\newcommand{\\note}[1]{}\n$-b$ and \\[ a  + b \\]\n"
        );
    }

    #[test]
    fn invocation_in_text_inside_math_is_replaced() {
        let input = b"\\newcommand{\\note}[1]{}\n$a \\text{if \\note{x} b}$\n";

        assert_eq!(
            clean(input),
            "\\newcommand{\\note}[1]{}\n$a \\text{if {} b}$\n"
        );
    }

    #[test]
    fn deletion_at_start_of_file() {
        assert_eq!(clean(b"\\iffalse x\\fi\nText\n"), "Text\n");
        assert_eq!(clean(b"\\usepackage{comment}\nText\n"), "Text\n");
        assert_eq!(
            clean(b"\\begin{comment}\nx\n\\end{comment}\nText\n"),
            "Text\n"
        );
    }

    #[test]
    fn deletion_at_end_of_file_without_newline() {
        assert_eq!(clean(b"Text\n\\iffalse x\\fi"), "Text\n");
        // The indentation of the deleted line is removed as well
        assert_eq!(clean(b"Text\n  \\iffalse x\\fi"), "Text\n");
    }

    #[test]
    fn consecutive_deleted_lines() {
        assert_eq!(clean(b"A\n\\iffalse x\\fi\n\\iffalse y\\fi\nB\n"), "A\nB\n");
        // The blank line between the deleted lines is a paragraph break and kept
        assert_eq!(
            clean(b"A\n\\iffalse x\\fi\n\n\\iffalse y\\fi\nB\n"),
            "A\n\nB\n"
        );
    }

    #[test]
    fn comment_package_followed_directly_by_another_line() {
        assert_eq!(
            clean(b"\\usepackage{comment}\n\\usepackage{x}\n"),
            "\\usepackage{x}\n"
        );
    }

    #[test]
    fn invocation_after_control_word_keeps_space() {
        // Deleting the invocation would let `\LaTeX` swallow the following space
        let input = b"\\newcommand{\\note}[1]{}\n\\LaTeX\\note{x} is\n";

        assert_eq!(clean(input), "\\newcommand{\\note}[1]{}\n\\LaTeX{} is\n");
    }

    #[test]
    fn invocation_in_math_environments_is_deleted() {
        let input = b"\\newcommand{\\note}[1]{}\n\\begin{align} a \\note{x} - b \\end{align}\n\\( c \\note{y} - d \\)\n";

        assert_eq!(
            clean(input),
            "\\newcommand{\\note}[1]{}\n\\begin{align} a  - b \\end{align}\n\\( c  - d \\)\n"
        );
    }

    #[test]
    fn indented_long_definition_on_own_line() {
        let input = b"Text\n  \\long\\def\\note#1{}\nMore \\note{x}\n";

        // The definition is kept, only the invocation is removed
        assert_eq!(clean(input), "Text\n  \\long\\def\\note#1{}\nMore {}\n");
    }
}
