#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::helper::ContainsByteSlice;
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
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
        assert!(_deletion_stats.grammar_errors.is_empty());
    }

    #[test]
    fn unbalanced_math_delimiters_in_environment_definition() {
        let input = br#"
\renewenvironment{cases}{%
  \left\{\def\arraystretch{1.2}% remove me
  \array{@{}l@{\quad}l@{}}%
}{%
  \endarray\right.%
}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"\left\{"));
        assert!(stripped.contains_slice(br"\right."));
        assert!(!stripped.contains_slice(b"remove me"));
        assert!(_deletion_stats.grammar_errors.is_empty());
    }

    #[test]
    fn unbalanced_math_delimiters_in_def() {
        let input = br#"
\def\open{\left( x}
\def\close{y \right)}
\newcommand{\bigopen}{\bigl[}
$\open \close \bigopen z \bigr]$"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stripped.is_some());
        assert!(_deletion_stats.grammar_errors.is_empty());
    }

    #[test]
    fn nested_math_delimiters() {
        let input = br#"$\left( a \left[ b \right] c \right)$ text % remove me"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"\left( a \left[ b \right] c \right)"));
        assert!(!stripped.contains_slice(b"remove me"));
        assert!(_deletion_stats.grammar_errors.is_empty());
    }

    #[test]
    fn def_with_delimited_parameters() {
        let input = br#"
\def\tik#1:{%
  \putrule from #1 -2 to #1 +2}
\def\Tik#1,#2:{% remove me
  \put {$#2$} [b] at #1 5}
\def\ax(#1){\hbox{({#1})}}
\def\foo#1\par{#1}
$\ax(I1)$ \Tik 1,2:"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"\def\tik#1:{"));
        assert!(stripped.contains_slice(br"\def\Tik#1,#2:{"));
        assert!(stripped.contains_slice(br"\def\ax(#1){\hbox{({#1})}}"));
        assert!(stripped.contains_slice(br"\def\foo#1\par{#1}"));
        assert!(!stripped.contains_slice(b"remove me"));
        assert!(_deletion_stats.comment_definition_deleted.is_empty());
        assert!(_deletion_stats.grammar_errors.is_empty());
    }

    #[test]
    fn def_without_parameters_is_cleaned() {
        let input = br#"\def\foo{bar % remove me
}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br"\def\foo{bar"));
        assert!(!stripped.contains_slice(b"remove me"));
        assert!(_deletion_stats.grammar_errors.is_empty());
    }

    #[test]
    fn newcommand_for_active_character() {
        let input = br#"
\catcode`\"\active
\newcommand{"}[2]{{}_{#1}{#2}}
\newcommand{\sumo}[1]{\ldots+"2{#1}+"1{#1}+"0{#1}}"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        let stripped = stripped.unwrap();
        assert!(stripped.contains_slice(br#"\newcommand{"}[2]{{}_{#1}{#2}}"#));
        assert!(_deletion_stats.grammar_errors.is_empty());
    }
}
