mod common;

/// Content the parser cannot match is copied without cleaning, thus parse errors
/// must stop the cleaner unless it is forced.
#[cfg(test)]
mod tests {
    use crate::common::{run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::deletion_stats::DeletionStats;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    /// A missing `\fi` after an `\else` turns the whole document body into one ERROR node.
    /// pdflatex still compiles the document, so the error is easily missed.
    const MISSING_FI_WITH_ELSE: &[u8] = br"\documentclass{article}
\newif\ifdraft
\draftfalse
\begin{document}
First paragraph. % secret comment 1
\ifdraft
Draft. % secret comment 2
\else
Final. % secret comment 3
\end{document}
";

    /// The parser recovers from a missing `\fi` in the preamble by inserting a MISSING node.
    const MISSING_FI_IN_PREAMBLE: &[u8] = br"\documentclass{article}
\newif\ifdraft
\drafttrue
\ifdraft
\usepackage{xcolor} % preamble comment
\begin{document}
Text. % secret comment
\end{document}
";

    /// An unclosed environment also turns the whole document body into one ERROR node.
    const UNCLOSED_ENVIRONMENT: &[u8] = br"\documentclass{article}
\begin{document}
\begin{itemize}
\item a % secret comment
\end{document}
";

    fn clean(input: &[u8], force: bool) -> anyhow::Result<(Option<Vec<u8>>, DeletionStats)> {
        let tree = parse(input).unwrap();
        ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig {
                force,
                ..Default::default()
            }),
        )
    }

    #[test]
    fn missing_fi_with_else_is_an_error() {
        let err = clean(MISSING_FI_WITH_ELSE, false).unwrap_err().to_string();

        assert!(err.contains("1 grammar error(s) in test.tex"), "{err}");
        assert!(err.contains("test.tex:4:0: cannot parse"), "{err}");
        assert!(err.contains("--force"), "{err}");
    }

    #[test]
    fn missing_fi_in_preamble_is_an_error() {
        let err = clean(MISSING_FI_IN_PREAMBLE, false)
            .unwrap_err()
            .to_string();

        // The parser inserts the `\fi` at the end of the file, the error points to the `\ifdraft`
        assert!(
            err.contains(r"test.tex:9:0: missing `\fi` to close"),
            "{err}"
        );
        assert!(err.contains(r#""\\ifdraft..." at 4:0"#), "{err}");
    }

    #[test]
    fn unclosed_environment_is_an_error() {
        let err = clean(UNCLOSED_ENVIRONMENT, false).unwrap_err().to_string();

        assert!(err.contains("test.tex:2:0: cannot parse"), "{err}");
    }

    #[test]
    fn forced_cleaning_reports_grammar_errors() {
        for input in [
            MISSING_FI_WITH_ELSE,
            MISSING_FI_IN_PREAMBLE,
            UNCLOSED_ENVIRONMENT,
        ] {
            let (stripped, stats) = clean(input, true).unwrap();

            assert!(stripped.is_some());
            assert_eq!(stats.grammar_errors.len(), 1, "{:?}", stats.grammar_errors);
        }
    }

    #[test]
    fn errors_in_removed_tail_are_ignored() {
        let input = br"\documentclass{article}
\begin{document}
Text. % comment
\end{document}
Notes \begin{itemize} }
";
        let (stripped, stats) = clean(input, false).unwrap();

        assert!(stats.grammar_errors.is_empty());
        assert_eq!(
            String::from_utf8(stripped.unwrap()).unwrap(),
            "\\documentclass{article}\n\\begin{document}\nText. %\n\\end{document}"
        );
    }

    #[test]
    fn errors_in_kept_tail_are_reported() {
        let input = br"% !ALC-NG keep-tail
\documentclass{article}
\begin{document}
Text. % comment
\end{document}
Notes \begin{itemize} }
";

        assert!(clean(input, false).is_err());
    }

    #[test]
    fn errors_in_files_kept_as_is_are_ignored() {
        let mut input = b"% !ALC-NG keep-all\n".to_vec();
        input.extend_from_slice(MISSING_FI_WITH_ELSE);

        let (stripped, stats) = clean(&input, false).unwrap();

        assert!(stats.grammar_errors.is_empty());
        assert_eq!(stripped.unwrap(), input);
    }

    #[test]
    fn valid_documents_have_no_grammar_errors() {
        let input = br"\documentclass{article}
\newif\ifdraft
\begin{document}
\ifdraft Draft. % comment
\else Final. \fi
\end{document}
";
        let (stripped, stats) = clean(input, false).unwrap();

        assert!(stats.grammar_errors.is_empty());
        assert!(stripped.is_some());
    }

    /// The submission fails without `force` and does not create a cleaned copy of the file.
    #[test]
    fn submission_with_grammar_error_fails_without_force() {
        let project = write_project(&[("main.tex", MISSING_FI_WITH_ELSE)]);

        let (_, target, result) = run_cleaner(
            "unparsable_content_cleaned",
            project.path(),
            CleanerConfig {
                skip_watermark: true,
                ..Default::default()
            },
        );

        let err = result.unwrap_err().to_string();
        assert!(err.contains("grammar error(s)"), "{err}");
        assert!(!target.join("main.tex").exists());
    }

    #[test]
    fn submission_with_grammar_error_is_cleaned_with_force() {
        let project = write_project(&[("main.tex", MISSING_FI_WITH_ELSE)]);

        let (submission, target, result) = run_cleaner(
            "unparsable_content_forced_cleaned",
            project.path(),
            CleanerConfig {
                force: true,
                skip_watermark: true,
                ..Default::default()
            },
        );

        result.unwrap();
        assert_eq!(submission.stats().grammar_errors.len(), 1);
        assert!(target.join("main.tex").is_file());
    }
}
