mod common;

#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::deletion_stats::DeletionStats;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    fn clean_with_stats(input: &[u8]) -> (String, DeletionStats) {
        let tree = parse(input).unwrap();
        let (stripped, stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(stats.grammar_errors.is_empty());
        (String::from_utf8(stripped.unwrap()).unwrap(), stats)
    }

    fn clean(input: &[u8]) -> String {
        clean_with_stats(input).0
    }

    /// Asserts that the cleaner keeps the input unchanged.
    fn assert_unchanged(input: &[u8]) {
        assert_eq!(clean(input), String::from_utf8_lossy(input));
    }

    fn config() -> CleanerConfig {
        CleanerConfig {
            force: true,
            skip_watermark: true,
            ..Default::default()
        }
    }

    #[test]
    fn invocation_payload_is_removed_and_definition_kept() {
        let (cleaned, stats) = clean_with_stats(b"\\newcommand{\\note}[1]{}\nA \\note{secret} B\n");

        assert_eq!(cleaned, "\\newcommand{\\note}[1]{}\nA {} B\n");
        assert_eq!(stats.comment_definitions.len(), 1);
        assert_eq!(stats.comment_invocation_deleted, vec![br"\note{secret}"]);
    }

    #[test]
    fn def_with_several_parameters() {
        assert_eq!(
            clean(b"\\def\\note#1#2{}\nA \\note{x}{y} B\n"),
            "\\def\\note#1#2{}\nA {} B\n"
        );
    }

    #[test]
    fn arguments_beyond_arity_are_kept() {
        // The second group is not consumed by `\note` and thus typeset
        assert_eq!(
            clean(b"\\newcommand{\\note}[1]{}\nA \\note{secret}{shown} B\n"),
            "\\newcommand{\\note}[1]{}\nA {}{shown} B\n"
        );
    }

    #[test]
    fn todo_with_optional_argument() {
        assert_eq!(
            clean(b"\\renewcommand{\\todo}[2][]{}\nA \\todo[inline]{x} \\todo{y} B\n"),
            "\\renewcommand{\\todo}[2][]{}\nA {} {} B\n"
        );
        // Without a default value, the brackets are the (only) argument and the group is typeset
        assert_unchanged(b"\\renewcommand{\\todo}[1]{}\nA \\todo[inline]{x} B\n");
    }

    #[test]
    fn command_invoking_only_comment_commands() {
        assert_eq!(
            clean(b"\\newcommand{\\a}[1]{}\n\\newcommand{\\b}[1]{\\a{#1}}\nX \\b{secret} Y\n"),
            "\\newcommand{\\a}[1]{}\n\\newcommand{\\b}[1]{{}}\nX {} Y\n"
        );
    }

    #[test]
    fn invocation_in_alignment_is_deleted() {
        // An empty group would start a cell and break `\hline`
        assert_eq!(
            clean(b"\\newcommand{\\note}[1]{}\n\\begin{tabular}{l}\na \\\\ \\note{q}\\hline\n\\end{tabular}\n"),
            "\\newcommand{\\note}[1]{}\n\\begin{tabular}{l}\na \\\\ \\hline\n\\end{tabular}\n"
        );
    }

    #[test]
    fn zero_arity_commands_are_kept() {
        // `\mt{text}` typesets the text, as `\mt` takes no argument (arXiv 1502.05046)
        assert_unchanged(b"\\newcommand{\\mt}{}\nA \\mt{visible text} B\n");
        // Flag tested by name (arXiv 1606.09160)
        assert_unchanged(b"\\newcommand{\\INCLUDEFIGS}{}\n\\@ifundefined{INCLUDEFIGS}{}{FIG}\n");
    }

    #[test]
    fn hooks_and_state_macros_are_kept() {
        // Definitions with an empty body change the output even without explicit invocation
        assert_unchanged(
            br"\makeatletter
\renewcommand{\@chapapp}{}
\chapter{A}
\renewcommand{\@chapapp}{Chapitre}
\def\@oddfoot{}\def\@evenfoot{}
\renewcommand{\thefootnote}{}
\renewcommand{\qedsymbol}{}
\def\@citea{}\@citea\def\@citea{,}
\newcommand{\ESO@HookI}{}
\newcommand{\AddPic}{\g@addto@macro\ESO@HookI}
\def\@biblabel#1{}
\@biblabel{x}
\makeatother
",
        );
    }

    #[test]
    fn manipulated_commands_are_kept() {
        assert_unchanged(b"\\newcommand{\\note}[1]{}\n\\let\\b\\note\nA \\note{x} B\n");
        assert_unchanged(b"\\newcommand{\\note}[1]{}\n\\expandafter\\note\\x\nA \\note{x} B\n");
        assert_unchanged(b"\\newcommand{\\note}[1]{}\n\\robustify{\\note}\nA \\note{x} B\n");
        assert_unchanged(b"\\newcommand{\\note}[1]{}\n\\g@addto@macro\\note{y}\nA \\note{x} B\n");
        assert_unchanged(b"\\newcommand{\\note}[1]{}\n\\csname note\\endcsname\nA \\note{x} B\n");
    }

    #[test]
    fn redefined_commands_are_kept() {
        assert_unchanged(
            b"\\newcommand{\\note}[1]{}\nA \\note{x}\n\\renewcommand{\\note}[1]{#1}\nB \\note{y}\n",
        );
        assert_unchanged(b"\\newcommand{\\note}[1]{}\n\\let\\note\\relax\nA \\note{x}\n");
    }

    #[test]
    fn scoped_and_conditional_definitions_are_kept() {
        assert_unchanged(b"{\\renewcommand{\\note}[1]{}\nA \\note{x}}\n");
        assert_unchanged(b"\\newcommand{\\wrap}{\\renewcommand{\\note}[1]{}}\nA \\note{x}\n");
        assert_unchanged(b"\\@ifclassloaded{memoir}{\\renewcommand\\note[1]{}}{}\nA \\note{x}\n");
    }

    #[test]
    fn invocation_before_definition_is_kept() {
        assert_eq!(
            clean(b"A \\note{x}\n\\newcommand{\\note}[1]{}\nB \\note{y}\n"),
            "A \\note{x}\n\\newcommand{\\note}[1]{}\nB {}\n"
        );
    }

    #[test]
    fn invocations_with_unbraced_or_optional_arguments_are_kept() {
        assert_unchanged(b"\\newcommand{\\note}[1]{}\nA \\note x B\n");
        assert_unchanged(b"\\newcommand{\\note}[1]{}\nA \\note[x]{y} B\n");
        assert_unchanged(b"\\newcommand{\\note}[2][]{}\nA \\note[x]{y} B\n");
        assert_unchanged(b"\\newcommand{\\note}[1]{}\nA \\note*{y} B\n");
    }

    #[test]
    fn ineligible_definitions_are_kept() {
        // Internal commands are never custom comment commands
        assert_unchanged(b"\\def\\my@note#1{}\nA \\my@note{x} B\n");
        // `\providecommand` keeps an existing definition
        assert_unchanged(b"\\providecommand{\\note}[1]{}\nA \\note{x} B\n");
        // Delimited parameters swallow text that is not a group
        assert_unchanged(b"\\def\\note#1\\par{}\nA \\note{x} B\n");
    }

    #[test]
    fn keep_comments_directive_keeps_invocations() {
        assert_unchanged(b"% !ALC-NG keep-comments\n\\newcommand{\\note}[1]{}\nA \\note{x} B\n");
    }

    /// Hooks, flags and state macros with an empty body keep the PDF unchanged.
    #[test]
    fn empty_hooks_keep_pdf() {
        let project = write_project(&[(
            "main.tex",
            br"\documentclass{report}
\usepackage{amsthm}
\newcommand{\INCLUDEFIGS}{}
\newcommand{\mt}{}
\newcommand{\nofn}[1]{{\renewcommand\thefootnote{}\footnote{#1}}}
\begin{document}
\makeatletter\renewcommand{\@chapapp}{}\makeatother
\chapter{First}
\makeatletter\renewcommand{\@chapapp}{Chapitre}\makeatother
\chapter{Second}
\mt{Highlighted text stays.}
\makeatletter\@ifundefined{INCLUDEFIGS}{No figures.}{Figures.}\makeatother
Text\nofn{An unnumbered footnote.}
\begin{proof}\renewcommand{\qedsymbol}{}Without symbol.\end{proof}
\begin{proof}With symbol.\end{proof}
\end{document}
",
        )]);

        let (submission, target, result) =
            run_cleaner("comment_commands_hooks_cleaned", project.path(), config());

        result.unwrap();
        let cleaned = std::fs::read_to_string(target.join("main.tex")).unwrap();
        assert!(cleaned.contains("Highlighted text stays."));
        assert!(is_pixel_identical(&submission));
    }

    /// Removed invocations keep the PDF unchanged, also in alignments and with further groups.
    #[test]
    fn removed_invocations_keep_pdf() {
        let project = write_project(&[(
            "main.tex",
            br"\documentclass{article}
\usepackage{todonotes}
\newcommand{\note}[1]{}
\def\two#1#2{}
\renewcommand{\todo}[2][]{}
\begin{document}
A \note{secret} B \note{secret}{ shown} C \two{secret}{secret} D.
E \todo[inline]{secret} F \todo{secret}.
\begin{tabular}{ll}
a & b \\
\note{secret}\hline
c & \note{secret} d \\ \hline
\end{tabular}
\end{document}
",
        )]);

        let (submission, target, result) = run_cleaner(
            "comment_commands_invocations_cleaned",
            project.path(),
            config(),
        );

        result.unwrap();
        let cleaned = std::fs::read_to_string(target.join("main.tex")).unwrap();
        assert!(!cleaned.contains("secret"));
        assert!(cleaned.contains("{ shown}"));
        assert!(is_pixel_identical(&submission));
    }

    /// A comment command defined in one file is removed in another.
    #[test]
    fn definition_and_invocation_in_different_files() {
        let project = write_project(&[
            (
                "main.tex",
                br"\documentclass{article}
\input{macros}
\begin{document}
\input{body}
\end{document}
",
            ),
            ("macros.tex", b"\\newcommand{\\note}[1]{}\n"),
            ("body.tex", b"Text \\note{secret} more.\n"),
        ]);

        let (submission, target, result) =
            run_cleaner("comment_commands_files_cleaned", project.path(), config());

        result.unwrap();
        let macros = std::fs::read_to_string(target.join("macros.tex")).unwrap();
        let body = std::fs::read_to_string(target.join("body.tex")).unwrap();
        assert_eq!(macros, "\\newcommand{\\note}[1]{}\n");
        assert_eq!(body, "Text {} more.\n");
        assert!(is_pixel_identical(&submission));
    }
}
