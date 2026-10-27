mod common;

#[cfg(test)]
mod tests {
    use crate::common::{is_pixel_identical, run_cleaner, write_project};

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::helper::ContainsByteSlice;

    const MAIN: &[u8] = br"\documentclass{article}
\begin{document}
Hello World! % a comment
\end{document}
";

    const OTHER: &[u8] = br"\documentclass{article}
\begin{document}
Other document.
\end{document}
";

    fn config() -> CleanerConfig {
        CleanerConfig {
            force: true,
            skip_watermark: true,
            ..Default::default()
        }
    }

    /// A classic `00README.XXX` that only sets global options (here `nostamp`)
    /// declares no top-level file, so the main files are detected automatically.
    #[test]
    fn detects_main_files_without_toplevel_entries() {
        let project = write_project(&[("main.tex", MAIN), ("00README.XXX", b"nostamp\n")]);

        let (submission, target, result) =
            run_cleaner("zzrm_options_only_cleaned", project.path(), config());
        result.unwrap();

        assert!(submission.zzrm().is_some());
        let main = std::fs::read(target.join("main.tex")).unwrap();
        assert!(main.contains_slice(b"Hello World!"));
        assert!(!main.contains_slice(b"a comment"));

        assert!(is_pixel_identical(&submission));
    }

    /// A `00README.md` with documentation is not an arXiv 00README.
    #[test]
    fn documentation_readme_does_not_affect_detection() {
        let project = write_project(&[
            ("main.tex", MAIN),
            (
                "00README.md",
                b"# My paper\n\nCompile with:\n\n    pdflatex main.tex\n\nSee the paper for details.\n",
            ),
        ]);

        let (submission, target, result) =
            run_cleaner("zzrm_documentation_cleaned", project.path(), config());
        result.unwrap();

        assert!(target.join("main.tex").is_file());
        assert!(is_pixel_identical(&submission));
    }

    /// A top-level file that does not exist must not silently produce an empty output.
    #[test]
    fn missing_toplevel_file_is_not_silently_ignored() {
        let project = write_project(&[
            ("main.tex", MAIN),
            ("00README.XXX", b"missing.tex toplevelfile\n"),
        ]);

        let (_, target, result) =
            run_cleaner("zzrm_missing_toplevel_cleaned", project.path(), config());

        assert!(
            result.is_err() || target.join("main.tex").is_file(),
            "the run succeeded without any output"
        );
    }

    /// Main files given by the user are compiled if the 00README declares no top-level file.
    #[test]
    fn user_main_files_with_options_only_readme() {
        let project = write_project(&[
            ("main.tex", MAIN),
            ("other.tex", OTHER),
            ("00README.XXX", b"nostamp\n"),
        ]);

        let (_, target, result) = run_cleaner(
            "zzrm_user_mains_options_cleaned",
            project.path(),
            CleanerConfig {
                user_provided_main_files: Some(vec!["other.tex".into()]),
                ..config()
            },
        );
        result.unwrap();

        assert!(target.join("other.tex").is_file());
        assert!(!target.join("main.tex").exists());
    }

    /// The top-level files of a 00README take precedence over the main files given by the user.
    #[test]
    fn readme_toplevel_files_take_precedence_over_user_main_files() {
        let project = write_project(&[
            ("main.tex", MAIN),
            ("other.tex", OTHER),
            ("00README.XXX", b"main.tex toplevelfile\n"),
        ]);

        let (_, target, result) = run_cleaner(
            "zzrm_user_mains_toplevel_cleaned",
            project.path(),
            CleanerConfig {
                user_provided_main_files: Some(vec!["other.tex".into()]),
                ..config()
            },
        );
        result.unwrap();

        assert!(target.join("main.tex").is_file());
        assert!(!target.join("other.tex").exists());
    }
}
