// Not every test crate uses every helper.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use alc_ng::cleaner::config::CleanerConfig;
use alc_ng::cleaner::submission::Submission;
use temp_dir::TempDir;

/// Returns the shared output directory for the full-pipeline tests, creating it if needed.
///
/// All tests that run the cleaner on a fixture write their results here
/// (each into its own subdirectory). The folder is gitignored.
pub fn test_output_dir() -> PathBuf {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("test_output");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Encodes a 16x16 PNG of a single color.
pub fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(16, 16, image::Rgb(rgb))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// Writes the files (relative path and content) into a new temporary project directory.
pub fn write_project(files: &[(&str, &[u8])]) -> TempDir {
    let dir = TempDir::with_prefix("alc-ng-test-").unwrap();

    for (path, content) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    dir
}

/// Cleans the project at `input` into a fresh `test_output/<name>` directory.
/// The submission is returned together with the result of [`Submission::run`].
pub fn run_cleaner(
    name: &str,
    input: &Path,
    config: CleanerConfig,
) -> (Submission, PathBuf, anyhow::Result<()>) {
    let target_path = test_output_dir().join(name);
    let _ = std::fs::remove_dir_all(&target_path);

    let mut submission =
        Submission::new(Arc::new(config), input, &target_path, "latexmk", &vec![]).unwrap();
    let result = submission.run();

    (submission, target_path, result)
}

/// Returns `true` if at least one main file was compiled and all main files compile
/// before and after cleaning to pixel-identical PDFs.
pub fn is_pixel_identical(submission: &Submission) -> bool {
    // Loading pdfium from several threads at once crashes the test process
    static PDFIUM: Mutex<()> = Mutex::new(());
    let comparison = {
        let _guard = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
        submission.compare().unwrap()
    };

    !comparison.is_empty()
        && comparison
            .into_values()
            .all(|res| res.is_some_and(|r| r.is_empty()))
}
