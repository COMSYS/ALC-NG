mod common;

#[cfg(test)]
mod tests {
    use crate::common::test_output_dir;

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::Submission;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Runs the full cleaning pipeline in provided-fls mode, i.e. without
    /// ever invoking a LaTeX compiler. This is the mode that is mandatory
    /// on wasm builds, where no subprocesses can be spawned.
    #[test]
    fn cleans_from_provided_fls_without_compiling() {
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let config = Arc::new(CleanerConfig {
            force: true,
            skip_watermark: true,
            fls_provided: Some(root.join("tests/provided_fls/main.fls")),
            ..Default::default()
        });

        let test_files = root.join("tests/provided_fls");
        let target_path = test_output_dir().join("provided_fls_cleaned");
        let _ = std::fs::remove_dir_all(&target_path);
        let mut submission =
            Submission::new(config, test_files, target_path, "latexmk", &vec![]).unwrap();

        submission.run().unwrap();

        let target = submission.target_path();
        // Files recorded as inputs in the provided .fls are copied ...
        assert!(target.join("main.tex").exists());
        assert!(target.join("used.tex").exists());
        // ... while unused files and the recorder file itself are dropped.
        assert!(!target.join("unused.tex").exists());
        assert!(!target.join("main.fls").exists());
    }

    /// A recorder file recorded on another machine — the author's own latexmk
    /// run, say — lists the main file with an absolute path from that machine,
    /// which cannot resolve inside the submission. The user-provided main file
    /// must therefore be kept regardless of what the recorder resolves, or the
    /// cleaned project would silently ship without its main file.
    #[test]
    fn keeps_user_provided_main_absent_from_foreign_fls() {
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let src = root.join("tests/provided_fls");

        // Isolated submission: the pipeline parses every .fls in the tree, so
        // only the foreign-path recorder may be present.
        let submission_dir = test_output_dir().join("provided_fls_foreign_submission");
        let _ = std::fs::remove_dir_all(&submission_dir);
        std::fs::create_dir_all(&submission_dir).unwrap();
        for name in ["main.tex", "used.tex", "unused.tex"] {
            std::fs::copy(src.join(name), submission_dir.join(name)).unwrap();
        }
        std::fs::copy(
            src.join("main_foreign_paths.fls"),
            submission_dir.join("main.fls"),
        )
        .unwrap();

        let config = Arc::new(CleanerConfig {
            force: true,
            skip_watermark: true,
            fls_provided: Some(submission_dir.join("main.fls")),
            user_provided_main_files: Some(vec![PathBuf::from("main.tex")]),
            ..Default::default()
        });

        let target_path = test_output_dir().join("provided_fls_foreign_cleaned");
        let _ = std::fs::remove_dir_all(&target_path);
        let mut submission =
            Submission::new(config, submission_dir, target_path, "latexmk", &vec![]).unwrap();

        submission.run().unwrap();

        let target = submission.target_path();
        // The main file survives even though only its foreign absolute path
        // is recorded; so does the relatively-recorder input.
        assert!(target.join("main.tex").exists());
        assert!(target.join("used.tex").exists());
        assert!(!target.join("unused.tex").exists());
    }
}
