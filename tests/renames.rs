mod common;

#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, png, run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::helper::ContainsByteSlice;
    use std::path::Path;

    const BIB: &[u8] = br"@book{knuth,
  author = {Donald E. Knuth},
  title = {The TeXbook},
  publisher = {Addison-Wesley},
  year = {1984},
}
";

    fn config() -> CleanerConfig {
        CleanerConfig {
            force: true,
            skip_watermark: true,
            rename_files: true,
            ..Default::default()
        }
    }

    fn read(path: impl AsRef<Path>) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    #[test]
    fn bibliography_and_watermark_follow_renamed_main() {
        let project = write_project(&[
            (
                "main paper.tex",
                br"\documentclass{article}
\begin{document}
See \cite{knuth}.
\bibliographystyle{plain}
\bibliography{refs}
\end{document}
",
            ),
            ("refs.bib", BIB),
        ]);

        let (submission, target, result) = run_cleaner(
            "renames_bibliography_cleaned",
            project.path(),
            CleanerConfig {
                skip_watermark: false,
                ..config()
            },
        );
        result.unwrap();

        assert!(!target.join("main paper.tex").exists());
        assert!(!target.join("main paper.bbl").exists());

        let main = read(target.join("main_paper.tex"));
        assert!(main.contains_slice(b"% Processed with alc-ng"));

        // The generated bibliography is shipped under the name of the renamed main file
        assert!(read(target.join("main_paper.bbl")).contains_slice(b"Knuth"));

        assert!(is_pixel_identical(&submission));
    }

    #[test]
    #[ignore = "main files in subdirectories produce an empty output, also without renaming"]
    fn main_file_in_renamed_folder() {
        let project = write_project(&[
            (
                "my dir/main paper.tex",
                br"\documentclass{article}
\begin{document}
See \cite{knuth}.
\bibliographystyle{plain}
\bibliography{refs}
\end{document}
",
            ),
            ("my dir/refs.bib", BIB),
        ]);

        // Main files are only detected in the project root
        let (submission, target, result) = run_cleaner(
            "renames_subdir_cleaned",
            project.path(),
            CleanerConfig {
                user_provided_main_files: Some(vec!["my dir/main paper.tex".into()]),
                ..config()
            },
        );
        result.unwrap();

        assert!(!target.join("my dir").exists());
        assert!(target.join("my_dir/main_paper.tex").is_file());
        assert!(read(target.join("my_dir/main_paper.bbl")).contains_slice(b"Knuth"));

        assert!(is_pixel_identical(&submission));
    }

    /// File usages of a 00README refer to the original names.
    #[test]
    fn zzrm_with_spaced_names() {
        let document = br"\documentclass{article}
\begin{document}
Hello World!
\end{document}
";
        let project = write_project(&[
            (
                "00README.json",
                br#"{"sources": [
  {"filename": "main paper.tex", "usage": "toplevel"},
  {"filename": "extra file.txt", "usage": "include"},
  {"filename": "notes file.txt", "usage": "ignore"}
]}"#,
            ),
            ("main paper.tex", document),
            ("other main.tex", document),
            ("extra file.txt", b"extra"),
            ("notes file.txt", b"notes"),
        ]);

        let (submission, target, result) =
            run_cleaner("renames_zzrm_cleaned", project.path(), config());
        result.unwrap();

        assert!(target.join("main_paper.tex").is_file());
        assert!(target.join("extra_file.txt").is_file());
        assert!(!target.join("notes_file.txt").exists());
        assert!(!target.join("notes file.txt").exists());
        // Only the top-level file of the 00README is compiled
        assert!(!target.join("other_main.tex").exists());

        assert!(is_pixel_identical(&submission));
    }

    /// A file is not renamed if its new name is already taken.
    #[test]
    fn collision_keeps_both_files() {
        let project = write_project(&[
            (
                "main.tex",
                br"\documentclass{article}
\begin{document}
\input{a b}
\input{a_b}
\end{document}
",
            ),
            ("a b.tex", b"Spaced.\n"),
            ("a_b.tex", b"Underscored.\n"),
        ]);

        let (submission, target, result) =
            run_cleaner("renames_collision_cleaned", project.path(), config());
        result.unwrap();

        assert_eq!(read(target.join("a b.tex")), b"Spaced.\n");
        assert_eq!(read(target.join("a_b.tex")), b"Underscored.\n");
        assert!(read(target.join("main.tex")).contains_slice(br"\input{a b}"));

        assert!(is_pixel_identical(&submission));
    }

    /// `my dir/x.png` keeps its name, as `my_dir/x.png` exists, but `my dir/y.png` is
    /// renamed. The search path `my dir/` must still find the original `x.png`.
    #[test]
    fn partial_folder_collision_keeps_references_correct() {
        let red = png([200, 30, 30]);
        let blue = png([30, 30, 200]);
        let project = write_project(&[
            (
                "main.tex",
                br"\documentclass{article}
\usepackage{graphicx}
\graphicspath{{my dir/}}
\begin{document}
\includegraphics[width=1cm]{x}
\includegraphics[width=1cm]{y}
\includegraphics[width=1cm]{my_dir/x}
\end{document}
",
            ),
            ("my dir/x.png", &red),
            ("my dir/y.png", &red),
            ("my_dir/x.png", &blue),
        ]);

        let (submission, _, result) = run_cleaner(
            "renames_partial_collision_cleaned",
            project.path(),
            config(),
        );
        result.unwrap();

        assert!(is_pixel_identical(&submission));
    }
}
