mod common;

#[cfg(test)]
mod tests {
    use crate::common::test_output_dir;

    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::Submission;
    use alc_ng::helper::ContainsByteSlice;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn clean(name: &str, tar: Option<&Path>) -> (Submission, PathBuf) {
        let config = Arc::new(CleanerConfig {
            force: true,
            skip_watermark: true,
            tar: tar.map(|t| t.to_string_lossy().to_string()),
            ..Default::default()
        });

        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let test_files = root.join("tests/spaces");
        let target_path = test_output_dir().join(name);
        let mut submission =
            Submission::new(config, test_files, &target_path, "latexmk", &vec![]).unwrap();

        submission.run().unwrap();

        (submission, target_path)
    }

    fn read(path: impl AsRef<Path>) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    #[test]
    fn keeps_spaces_without_tar() {
        let (submission, target) = clean("spaces_without_tar_cleaned", None);

        assert!(target.join("main paper.tex").is_file());
        assert!(target.join("my sections/part a.tex").is_file());
        assert!(target.join("figs/red square.png").is_file());

        let main = read(target.join("main paper.tex"));
        assert!(main.contains_slice(br"\input{my sections/part a}"));
        assert_eq!(
            read(target.join("frozen.tex")),
            read("tests/spaces/frozen.tex")
        );

        assert!(submission
            .compare()
            .unwrap()
            .into_iter()
            .all(|(_, res)| res.is_some_and(|r| r.is_empty())));
    }
}
