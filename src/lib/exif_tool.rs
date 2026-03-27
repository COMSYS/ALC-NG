use std::path::Path;

use anyhow::{Context, Result};

pub fn ensure_requirements(exiftool_cmd: &str) {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(exiftool_cmd);
    cmd.arg("--version");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let status = cmd.status().is_ok_and(|v| v.success());

    if !status {
        panic!("The exiftool command is not available. Please check the configuration.");
    }
}

pub fn clean_inplace(
    path: impl AsRef<Path>,
    exiftool_cmd: &str,
    exiftool_args: &[String],
) -> Result<()> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(exiftool_cmd);
    cmd.arg("-all:all=");
    cmd.args(["-tagsFromFile", "@", "-exif:Orientation"]);
    cmd.arg("-r");
    cmd.arg("-overwrite_original");
    cmd.args(exiftool_args);
    cmd.arg(path.as_ref());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    cmd.status().map(|_| ()).context("Failed to run exiftool")
}
