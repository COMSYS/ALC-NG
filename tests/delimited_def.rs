mod common;

#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

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

    /// An empty macro with delimited parameters swallows text up to its delimiter,
    /// which is not a curly group of the invocation. The swallowed text must not
    /// become visible: it may only remain together with the macro that hides it.
    #[test]
    fn empty_def_with_delimited_parameters_keeps_argument_hidden() {
        for (macro_name, input) in [
            (
                r"\skipto",
                &br"\def\skipto#1\par{}
\skipto hidden text

Visible
"[..],
            ),
            (
                r"\drop",
                &br"\def\drop(#1){}
Visible \drop(hidden) end
"[..],
            ),
        ] {
            let cleaned = clean(input);

            assert!(
                !cleaned.contains("hidden") || cleaned.contains(macro_name),
                "{macro_name}: hidden text is visible in {cleaned:?}"
            );
        }
    }

    #[test]
    fn empty_def_with_delimited_parameters_is_pixel_identical() {
        let project = write_project(&[(
            "main.tex",
            br"\documentclass{article}
\def\skipto#1\par{}
\def\drop(#1){}
\begin{document}
Visible one.
\skipto hidden text

Visible two \drop(hidden too) end.
\end{document}
",
        )]);

        let (submission, _, result) = run_cleaner(
            "delimited_def_cleaned",
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
