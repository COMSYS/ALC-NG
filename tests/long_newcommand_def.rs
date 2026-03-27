#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::helper::ContainsByteSlice;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn long_newcommand_def() {
        let input = br#"
\documentclass{article}

\long\newcommand\Ignore[1]{}

\begin{document}
Text  \Ignore{(Comment)}  Besides
\end{document}
"#;
        let tree = parse(input).unwrap();
        let (stripped, grammar_errors, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();

        assert!(!stripped.contains_slice(br"\Ignore"));
        assert!(stripped.contains_slice(br"Text  {}  Besides"));
        assert!(grammar_errors.len() == 0);
    }

    #[test]
    fn long_newcommand_def_with_curly_braces() {
        let input = br#"
\documentclass{article}

\long\newcommand{\Ignore}[1]{}

\begin{document}
Text  \Ignore{(Comment)}  Besides
\end{document}
"#;
        let tree = parse(input).unwrap();
        let (stripped, grammar_errors, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();

        assert!(!stripped.contains_slice(br"\Ignore"));
        assert!(stripped.contains_slice(b"Text  {}  Besides"));
        assert!(grammar_errors.len() == 0);
    }
}
