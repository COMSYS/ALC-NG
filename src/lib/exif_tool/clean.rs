pub fn clean_inplace(path: impl AsRef<Path>) -> Result<()> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(&CONFIG.exiftool_cmd);
    cmd.arg("-all:all=");
    cmd.arg("-r");
    cmd.arg("-overwrite_original");
    cmd.args(&CONFIG.exiftool_args);
    cmd.arg(path.as_ref());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    cmd.status().map(|_| ()).context("Failed to run exiftool")
}
