#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn comment_env_following_text_directly() {
        let input = br#"
Text\begin{comment}
    some comment
\end{comment}
after"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert_eq!(stripped, Some(Vec::from(b"Textafter")));
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn comment_env_with_newline_in_front() {
        let input = br#"Text
\begin{comment}
    some comment
\end{comment}
after"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert_eq!(stripped, Some(Vec::from("Text\nafter")));
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn comment_env_with_newlines_in_front_and_after() {
        let input = br#"Text


\begin{comment}
some comment
\end{comment}


after"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert_eq!(stripped, Some(Vec::from("Text\n\n\n\n\nafter")));
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }
}
