#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn newlines_between_linecomments() {
        let input = br#"\documentclass{article}


\begin{document}
% Comment 1
% Comment 2


% Comment 3
\end{document}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        let expected = br#"\documentclass{article}


\begin{document}


\end{document}"#;

        assert_eq!(stripped, expected);

        assert!(_deletion_stats.grammar_errors.len() == 0);
    }
}
