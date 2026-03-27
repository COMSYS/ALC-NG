#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::Submission;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn test_1806_91978v1() {
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
            ..Default::default()
        });

        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let test_files = root.join("tests/1806_01078v1");
        let target_path = root.join("1806_01078v1_cleaned");
        let mut submission =
            Submission::new(config.clone(), test_files, target_path, "latexmk", &vec![]).unwrap();

        submission.run().unwrap();

        assert!(
            submission
                .compare()
                .unwrap()
                .into_iter()
                .all(|(_, res)| res.is_empty())
        );
    }
}
