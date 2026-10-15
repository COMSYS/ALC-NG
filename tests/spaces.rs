#[cfg(test)]
mod tests {
    use alc_ng::cleaner::config::CleanerConfig;
    use alc_ng::cleaner::submission::Submission;
    use alc_ng::helper::ContainsByteSlice;
    use flate2::read::GzDecoder;
    use std::fs::File;
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
        let target_path = root.join(name);
        let mut submission =
            Submission::new(config, test_files, &target_path, "latexmk", &vec![]).unwrap();

        submission.run().unwrap();

        (submission, target_path)
    }

    fn read(path: impl AsRef<Path>) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    #[test]
    fn replaces_spaces_in_file_names() {
        let tar = std::env::temp_dir().join("alc-ng-spaces-test.tar.gz");
        let (submission, target) = clean("spaces_cleaned", Some(&tar));

        assert!(target.join("main_paper.tex").is_file());
        assert!(target.join("intro_section.tex").is_file());
        assert!(target.join("my_sections/part_a.tex").is_file());
        assert!(target.join("figs/red_square.png").is_file());
        assert!(target.join("figs/blue_square.png").is_file());
        assert!(!target.join("main paper.tex").exists());
        assert!(!target.join("my sections").exists());

        let main = read(target.join("main_paper.tex"));
        assert!(main.contains_slice(br"\input{intro_section}"));
        assert!(main.contains_slice(br"\input{my_sections/part_a}"));
        assert!(main.contains_slice(br"\includegraphics[width=1cm]{figs/red_square.png}"));
        assert!(main.contains_slice(br"\includegraphics[width=1cm]{blue_square}"));
        assert!(main.contains_slice(br"\graphicspath{{figs/}}"));
        assert!(!main.contains_slice(b"A comment"));

        // Files that are not cleaned only get their references rewritten
        let frozen = read(target.join("frozen.tex"));
        assert_eq!(
            frozen,
            b"% !ALC-NG keep-all\nFrozen: \\input{intro_section} % this comment is kept\n"
        );
        let logo = read(target.join("logo.sty"));
        assert!(logo.contains_slice(b"% A comment in an uncleaned package"));
        assert!(logo.contains_slice(br"\includegraphics[width=1cm]{figs/red_square}"));

        // The tar archive contains the renamed files
        let mut archive = tar::Archive::new(GzDecoder::new(File::open(&tar).unwrap()));
        let entries: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(entries.iter().any(|e| e == "main_paper.tex"));
        assert!(entries.iter().any(|e| e == "my_sections/part_a.tex"));
        assert!(entries.iter().all(|e| !e.contains(' ')));
        std::fs::remove_file(&tar).unwrap();

        // The renamed project compiles to the same PDF
        assert!(
            submission
                .compare()
                .unwrap()
                .into_iter()
                .all(|(_, res)| res.is_some_and(|r| r.is_empty()))
        );
    }
}
