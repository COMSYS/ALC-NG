#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn item_with_paranthesis_in_square_brackets() {
        let input = br#"
            \begin{itemize}
                \item[(i)] bar
                \item[(ii)] baz
            \end{itemize}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn author_with_square_brackets() {
        let input = br#"
\author{Christian Alber\footnotemark[1] \and Chupeng Ma\footnotemark[2]
				\and
                     Robert Scheichl\footnotemark[3]
                     }"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn input_without_curly_brackets() {
        let input = br#"\input asd"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn newcommand_with_env_inside() {
        let input = br#"\newcommand{\ben}{\begin{enumerate}}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn item_with_inline_math() {
        let input = br#"
            \begin{enumerate}
                \item [$B_1:$]
                \item [$$B_1:$$]
            \end{enumerate}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn item_with_curly_group() {
        let input = br#"
            \begin{enumerate}
                \item [{$B_1:$}]
                \item [{{$$B_1:$$}}]
            \end{enumerate}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn caption_cmd_in_newcommand() {
        let input = br#"\newcommand{\tabcaption}{\caption}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn label_with_parantheses() {
        let input = br#"\label{f(opt)-table}\ref{f(opt)-table}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn array_with_if() {
        let input = br#"$$\begin{array}{rc}
        \iff
        \end{array}$$
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }
}
