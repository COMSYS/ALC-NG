mod common;

#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    /// Cleans without `force`, so an unknown conditional is an error.
    fn clean(input: &[u8]) -> String {
        let tree = parse(input).unwrap();
        let (stripped, _) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        String::from_utf8(stripped.unwrap()).unwrap()
    }

    #[test]
    fn ifdefined_keeps_both_branches() {
        // arXiv 1606.09160
        assert_eq!(
            clean(b"\\newcommand{\\INCLUDEFIGS}{}\n\\ifdefined\\INCLUDEFIGS FIG\\else NOFIG\\fi\n"),
            "\\newcommand{\\INCLUDEFIGS}{}\n\\ifdefined\\INCLUDEFIGS FIG\\else NOFIG\\fi\n"
        );
    }

    #[test]
    fn branches_of_primitive_conditionals_are_cleaned() {
        assert_eq!(
            clean(b"\\ifdefined\\x A% secret\n\\else B% secret\n\\fi\n"),
            "\\ifdefined\\x A%\n\\else B%\n\\fi\n"
        );
    }

    #[test]
    fn other_primitive_conditionals() {
        for input in [
            &b"\\ifx\\a\\b same\\else different\\fi\n"[..],
            b"\\ifnum\\value{page}>1 later\\fi\n",
            b"\\ifmmode math\\else text\\fi\n",
            b"\\ifcase\\value{x} zero\\or one\\else many\\fi\n",
        ] {
            assert_eq!(clean(input), String::from_utf8_lossy(input));
        }
    }

    #[test]
    fn undefined_custom_conditional_is_still_an_error() {
        let input = b"\\ifdraft A\\fi\n";
        let tree = parse(input).unwrap();

        assert!(
            ContentStripper::clean(
                input,
                tree.root_node(),
                "test.tex",
                Arc::new(CleanerConfig::default()),
            )
            .is_err()
        );
    }

    /// The branches open and close environments, which the parser cannot pair (arXiv 2210.00025).
    /// The resulting ERROR node must not lose the `\ifdefined` or the `\else`.
    const PROOF_ENVIRONMENTS: &[u8] =
        br"\newenvironment{rproofof}[1]{ \ifdefined\informs \begin{proof}{\it Proof of #1.}
\else \begin{proof}[Proof of #1] \fi }{ \ifdefined\informs
\Halmos \end{proof} \else \end{proof} \fi  }

\newenvironment{rproof}{
\noindent \ifdefined\informs \proof{\it Proof.}
\else \begin{proof} \fi }{ \ifdefined\informs
\Halmos \endproof \else \end{proof} \fi}
";

    #[test]
    fn conditionals_in_unparsable_environments_are_kept() {
        assert_eq!(
            clean(PROOF_ENVIRONMENTS),
            String::from_utf8_lossy(PROOF_ENVIRONMENTS)
        );
    }

    #[test]
    fn conditionals_in_unparsable_environments_keep_pdf() {
        let mut main = br"\documentclass{article}
\usepackage{amsthm}
"
        .to_vec();
        main.extend_from_slice(PROOF_ENVIRONMENTS);
        main.extend_from_slice(
            br"\begin{document}
\begin{rproof}
A proof.
\end{rproof}
\begin{rproofof}{the lemma}
Another proof.
\end{rproofof}
\end{document}
",
        );
        let project = write_project(&[("main.tex", &main)]);

        let (submission, _, result) = run_cleaner(
            "primitive_conditionals_cleaned",
            project.path(),
            CleanerConfig {
                force: true,
                skip_watermark: true,
                ..Default::default()
            },
        );

        result.unwrap();
        assert!(is_pixel_identical(&submission));
    }
}
