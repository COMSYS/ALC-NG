#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn comment_env_with_windows_newline_in_front() {
        let input = b"Text\r\n\\begin{comment}\r\n    some comment\r\n\\end{comment}\r\nafter";
        let tree = parse(input).unwrap();
        let (stripped, grammar_errors, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert_eq!(stripped, Some(Vec::from(b"Text\r\nafter")));
        assert!(grammar_errors.len() == 0);
    }

    #[test]
    fn comment_env_with_windows_newlines_in_front_and_after() {
        let input =
            b"Text\r\n\r\n\r\n\\begin{comment}\r\nsome comment\r\n\\end{comment}\r\n\r\n\r\nafter";
        let tree = parse(input).unwrap();
        let (stripped, grammar_errors, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert_eq!(stripped, Some(Vec::from(b"Text\r\n\r\n\r\n\r\n\r\nafter")));
        assert!(grammar_errors.len() == 0);
    }

    #[test]
    fn line_comment_with_windows_newlines() {
        let input = b"Text\r\n% comment\r\nafter";
        let tree = parse(input).unwrap();
        let (stripped, grammar_errors, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert_eq!(stripped, Some(Vec::from(b"Text\r\nafter")));
        assert!(grammar_errors.len() == 0);
    }
}
