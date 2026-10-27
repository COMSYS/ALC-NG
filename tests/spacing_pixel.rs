mod common;

#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;

    fn config() -> CleanerConfig {
        CleanerConfig {
            force: true,
            skip_watermark: true,
            ..Default::default()
        }
    }

    /// Deletions of full lines and replaced invocations of comment commands must not
    /// change the spacing in the PDF.
    #[test]
    fn deletions_keep_spacing() {
        let project = write_project(&[
            (
                "main.tex",
                br"\documentclass{article}
\usepackage{comment}
\usepackage{amsmath}
\newcommand{\note}[1]{}
  \long\def\later#1{}
\begin{document}
\input{start}
\input{crlf}
Text before
    \iffalse hidden \fi
after the indented deletion.
\iffalse a\fi
\iffalse b\fi
Consecutive deletions.

Line with
\note{on its own line}
invocation.

\LaTeX\note{x} is typeset with its space.
\begin{align}
  a \note{x} - b
\end{align}
Inline \( c \note{y} - d \) math.
\end{document}
",
            ),
            ("start.tex", b"\\iffalse x\\fi\nStart of input.\n"),
            (
                "crlf.tex",
                b"Windows\r\n    \\iffalse x\\fi\r\nline endings.\r\n",
            ),
        ]);

        let (submission, target, result) =
            run_cleaner("spacing_pixel_cleaned", project.path(), config());

        result.unwrap();
        assert!(target.join("main.tex").is_file());
        assert!(is_pixel_identical(&submission));
    }

    /// `\mbox` typesets its argument in text mode, even inside a formula.
    #[test]
    fn invocation_in_mbox_inside_math_keeps_spacing() {
        let project = write_project(&[(
            "main.tex",
            br"\documentclass{article}
\newcommand{\note}[1]{}
\begin{document}
$\mbox{a \note{x} b}$
\end{document}
",
        )]);

        let (submission, _, result) = run_cleaner("spacing_mbox_cleaned", project.path(), config());

        result.unwrap();
        assert!(is_pixel_identical(&submission));
    }
}
