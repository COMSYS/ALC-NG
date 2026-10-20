use std::path::PathBuf;

/// Returns the shared output directory for the full-pipeline tests, creating it if needed.
///
/// All tests that run the cleaner on a fixture write their results here
/// (each into its own subdirectory). The folder is gitignored.
pub fn test_output_dir() -> PathBuf {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("test_output");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
