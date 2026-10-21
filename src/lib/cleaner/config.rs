use std::path::PathBuf;

// Wasm builds drive the cleaner through a JSON configuration handed to the
// `alc_ng_run` FFI entry point, which deserializes directly into this struct.
#[derive(Debug)]
#[cfg_attr(target_family = "wasm", derive(serde::Deserialize))]
#[cfg_attr(target_family = "wasm", serde(default))]
pub struct CleanerConfig {
    pub strip_exif: bool,
    /// Preserve .bib files
    pub keep_bib: bool,
    /// Resize images to preserve space
    pub resize_images: bool,
    /// Size of the output images (in pixels, longest side).
    /// Fine tune this to get as close to 10MB as possible.
    pub im_size: u32,
    /// Also clean .sty and .cls latex files.
    pub clean_classes: bool,
    /// Ignore the 00readme even though it is defined.
    pub no_zzrm: bool,
    /// Create a tar archive that is ready to be uploaded.
    /// If the value is '-' then the archive is piped to stdout.
    /// Spaces in file names are replaced with underscores (as arXiv does on upload)
    /// and references to the renamed files are updated.
    pub tar: Option<String>,
    /// Continue cleaning despite hitting errors.
    pub force: bool,
    /// The command for exiftool.
    pub exiftool_cmd: String,
    /// Additional parameters to pass to exiftool.
    pub exiftool_args: Vec<String>,
    /// Do not attach watermark to end of main files
    pub skip_watermark: bool,
    /// Main files override
    pub user_provided_main_files: Option<Vec<PathBuf>>,
    /// Path to a LaTeX recorder (.fls) file that replaces the compile step.
    /// When set, the submission is not compiled at all; the provided recorder
    /// file is used to determine the used files instead. This is required on
    /// wasm builds, which cannot spawn a LaTeX compiler.
    ///
    /// The recorder file is trusted as is: nothing is compiled to verify it,
    /// so a file missing from its `INPUT` lines is dropped from the output.
    pub fls_provided: Option<PathBuf>,
    /// Print full LaTeX compiler output on failure.
    pub verbose: bool,
}

impl Default for CleanerConfig {
    fn default() -> Self {
        Self {
            strip_exif: false,
            keep_bib: false,
            resize_images: false,
            im_size: 1024,
            clean_classes: false,
            no_zzrm: false,
            tar: None,
            force: false,
            exiftool_cmd: "exiftool".to_string(),
            exiftool_args: Vec::new(),
            skip_watermark: false,
            user_provided_main_files: None,
            fls_provided: None,
            verbose: false,
        }
    }
}
