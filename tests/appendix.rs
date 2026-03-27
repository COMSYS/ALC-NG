#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::Submission;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn test_appendix() {
        let config = Arc::new(CleanerConfig {
            force: true,
            clean_classes: false,
            exiftool_args: vec![],
            exiftool_cmd: "exiftool".to_string(),
            im_size: 512,
            keep_bib: false,
            no_zzrm: false,
            resize_images: false,
            strip_exif: false,
            tar: None,
            skip_watermark: true,
        });

        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let test_files = root.join("tests/appendix");
        let target_path = root.join("appendix_cleaned");
        let mut submission =
            Submission::new(config.clone(), test_files, target_path, "latexmk", &vec![]).unwrap();

        submission.run().unwrap();

        assert!(
            submission
                .compare()
                .unwrap()
                .into_iter()
                .all(|(_, res)| res.unwrap().iter().all(|v| *v))
        );
    }
}
