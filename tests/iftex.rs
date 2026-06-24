#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::helper::ContainsByteSlice;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn ifpdf() {
        let input = br#"
            \ifpdf
            if_content
            \else
            else_content
            \fi
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"if_content"));
        assert!(stripped.contains_slice(br"else_content"));
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn ifpdf_partial() {
        let input = br#"
            \ifpdf
            if_content
            \fi
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"if_content"));
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn ifpdf_partial_with_comments() {
        let input = br#"
            \ifpdf
            % comment
            if_content
            \fi
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"if_content"));
        assert!(!stripped.contains_slice(br"comment"));
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn ifpdf_with_comments() {
        let input = br#"
            \ifpdf
            % comment1
            \begin{itemize}
            % comment2
            \item content1
            \end{itemize}
            % comment3
            \else
            % comment4
            \begin{itemize}
            % comment5
            \item content2
            \end{itemize}
            \fi
            % comment6
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"content1"));
        assert!(stripped.contains_slice(br"content2"));

        assert!(!stripped.contains_slice(br"comment1"));
        assert!(!stripped.contains_slice(br"comment2"));
        assert!(!stripped.contains_slice(br"comment3"));
        assert!(!stripped.contains_slice(br"comment4"));
        assert!(!stripped.contains_slice(br"comment5"));
        assert!(!stripped.contains_slice(br"comment6"));

        assert!(_deletion_stats.grammar_errors.len() == 0);
    }
}
