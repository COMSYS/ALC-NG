#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::parsed_file::ContentStripper;
    use alc_ng::parsing::parse;
    use std::sync::Arc;

    fn clean(input: &[u8]) -> String {
        let tree = parse(input).unwrap();
        let (stripped, deletion_stats) = ContentStripper::clean(
            input,
            tree.root_node(),
            "test.tex",
            Arc::new(CleanerConfig::default()),
        )
        .unwrap();

        assert!(deletion_stats.grammar_errors.is_empty());
        String::from_utf8(stripped.unwrap()).unwrap()
    }

    #[test]
    fn invocation_before_hline_is_deleted() {
        // `{}` in front of \hline fails with "Misplaced \noalign"
        let input = br#"\newcommand{\note}[1]{}
\begin{tabular}{ll}
a & b \\
\note{fix}
\hline
c & d \\
  \note{fix}
\hline
\end{tabular}
"#;

        assert_eq!(
            clean(input),
            r#"\begin{tabular}{ll}
a & b \\
\hline
c & d \\
\hline
\end{tabular}
"#
        );
    }

    #[test]
    fn invocation_before_multicolumn_is_deleted() {
        // `{}` in front of \multicolumn fails with "Misplaced \omit"
        let input = br#"\newcommand{\note}[1]{}
\begin{tabular}{ll}
a & \note{fix}\multicolumn{1}{c}{b} \\
\end{tabular}
"#;

        assert_eq!(
            clean(input),
            r#"\begin{tabular}{ll}
a & \multicolumn{1}{c}{b} \\
\end{tabular}
"#
        );
    }

    #[test]
    fn invocations_at_cell_starts_are_deleted() {
        let input = br#"\newcommand{\note}[1]{}
\begin{tabular*}{\textwidth}[t]{ll}
\note{first} a & \note{second} b \\ % comment
\note{third}\note{fourth} c & d \tabularnewline
\midrule
\note{fifth} e & f
\end{tabular*}
"#;

        assert_eq!(
            clean(input),
            r#"\begin{tabular*}{\textwidth}[t]{ll}
 a &  b \\ %
 c & d \tabularnewline
\midrule
 e & f
\end{tabular*}
"#
        );
    }

    #[test]
    fn invocations_inside_cells_are_replaced() {
        // Within a cell, `{}` keeps the spacing of the original
        let input = br#"\newcommand{\note}[1]{}
\begin{tabular}{ll}
a \note{fix} b & {\note{fix} c} \\
\end{tabular}
Text \note{fix} text
"#;

        assert_eq!(
            clean(input),
            r#"\begin{tabular}{ll}
a {} b & {{} c} \\
\end{tabular}
Text {} text
"#
        );
    }
}
