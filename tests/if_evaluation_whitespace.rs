mod common;

/// The whitespace around evaluated conditionals must keep the meaning of the document.
///
/// TeX keeps the whitespace in front of a conditional and between the kept branch and the
/// following `\else` or `\fi`. It skips spaces and one newline after the control words
/// `\ifX`, `\else` and `\fi`. Thus, an evaluated conditional on its own lines is removed
/// together with these lines, but the lines in front of and after it stay separate.
#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    const DEFINITIONS: &str = "\\newif\\ifdraft\n\\drafttrue\n\\newif\\iffinal\n";

    /// Inputs (after the definitions) and their expected cleaned output.
    const CASES: &[(&str, &str, &str)] = &[
        (
            "words in front",
            "Text before\n\\ifdraft\nA\n\\else\nB\n\\fi\nafter\n",
            "Text before\nA\nafter\n",
        ),
        (
            "command in front",
            "\\noindent\n\\ifdraft\nC\n\\fi\nafter\n",
            "\\noindent\nC\nafter\n",
        ),
        (
            "paragraph in front",
            "Text\n\n\\ifdraft\nD\n\\fi\nafter\n",
            "Text\n\nD\nafter\n",
        ),
        (
            "paragraph after",
            "Text\n\\ifdraft\nD\n\\fi\n\nafter\n",
            "Text\nD\n\nafter\n",
        ),
        (
            "else branch",
            "Text\n\\iffinal\nA\n\\else\nB\n\\fi\nafter\n",
            "Text\nB\nafter\n",
        ),
        (
            "suffix comment in front",
            "Section text. % comment\n\\ifdraft\nAnonymous version.\n\\fi\nafter\n",
            "Section text. %\nAnonymous version.\nafter\n",
        ),
        ("inline", "Text \\ifdraft A \\fi more\n", "Text A more\n"),
        (
            "inline without spaces in the branch",
            "Text \\ifdraft A\\else B\\fi more\n",
            "Text Amore\n",
        ),
        (
            "environment after",
            "\\ifdraft\nA\n\\fi\n\\begin{itemize}\\item x\\end{itemize}\n",
            "A\n\\begin{itemize}\\item x\\end{itemize}\n",
        ),
        (
            "nested",
            "Text\n\\ifdraft\n\\iffinal\nA\n\\else\nB\n\\fi\n\\fi\nafter\n",
            "Text\nB\nafter\n",
        ),
        (
            "iftrue",
            "Text\n\\iftrue\nA\n\\fi\nafter\n",
            "Text\nA\nafter\n",
        ),
        (
            "iffalse with else",
            "Text\n\\iffalse\nA\n\\else\nB\n\\fi\nafter\n",
            "Text\nB\nafter\n",
        ),
    ];

    fn clean(input: &str) -> String {
        let input = input.as_bytes();
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
    fn whitespace_around_evaluated_conditionals() {
        let failures: Vec<_> = CASES
            .iter()
            .filter_map(|(name, input, expected)| {
                let cleaned = clean(&format!("{DEFINITIONS}{input}"));
                let expected = format!("{DEFINITIONS}{expected}");
                (cleaned != expected)
                    .then(|| format!("{name}:\n  expected {expected:?}\n  cleaned  {cleaned:?}"))
            })
            .collect();

        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    /// All cases in one document compile to the same pages after cleaning.
    #[test]
    fn evaluated_conditionals_keep_pdf() {
        let mut main = format!("\\documentclass{{article}}\n{DEFINITIONS}\\begin{{document}}\n");
        for (name, input, _) in CASES {
            main.push_str(&format!("\\section{{{name}}}\n{input}\n"));
        }
        main.push_str("\\end{document}\n");

        let project = write_project(&[("main.tex", main.as_bytes())]);
        let (submission, _, result) = run_cleaner(
            "if_evaluation_whitespace_cleaned",
            project.path(),
            CleanerConfig {
                skip_watermark: true,
                ..Default::default()
            },
        );

        result.unwrap();
        assert!(is_pixel_identical(&submission));
    }
}
