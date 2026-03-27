#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::helper::ContainsByteSlice;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    #[test]
    fn simple() {
        let input = br#"
            \newif\iffoo
            \foofalse
            \newcommand{\customdraft}[1]{\iffoo#1\else\fi}

            \customdraft{Text}
            Not in command
            \customdraft{Another Text}
            After
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(
            !stripped
                .unwrap()
                .contains_slice(br"\newcommand{\customdraft}")
        );
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn whitespace_and_comments() {
        let input = br#"
            \newif\iffoo
            \foofalse
            \newcommand{\customdraft}[1]{
                % I am just a comment
                \iffoo#1\else\fi
                % I am more

                % With whitespace beforehand
            }

            \customdraft{Text}
            Not in command
            \customdraft{Another Text}
            After
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(
            !stripped
                .unwrap()
                .contains_slice(br"\newcommand{\customdraft}")
        );
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }

    #[test]
    fn should_keep_command() {
        let input = br#"
            \newif\iffoo
            \foofalse
            \newcommand{\customdraft}[1]{
                % I am just a comment
                \iffoo#1\else\fi
                % I am more
                Some text
                % With whitespace beforehand
            }

            \customdraft{Text}
            Not in command
            \customdraft{Another Text}
            After
"#;
        let tree = parse(input).unwrap();
        let (stripped, _deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(
            stripped
                .unwrap()
                .contains_slice(br"\newcommand{\customdraft}")
        );
        assert!(_deletion_stats.grammar_errors.len() == 0);
    }
}
