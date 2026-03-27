#[derive(Debug)]
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
    pub tar: Option<String>,
    /// Continue cleaning despite hitting errors.
    pub force: bool,
    /// The command for exiftool.
    pub exiftool_cmd: String,
    /// Additional parameters to pass to exiftool.
    pub exiftool_args: Vec<String>,
    /// Do not attach watermark to end of main files
    pub skip_watermark: bool,
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
        }
    }
}
