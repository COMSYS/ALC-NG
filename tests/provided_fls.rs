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
}
