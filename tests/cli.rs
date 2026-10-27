mod common;

#[cfg(test)]
mod tests {
    use crate::common::test_output_dir;

    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    /// Without a terminal (e.g. in CI), `--tar` must not ask whether to rename files,
    /// which would block forever, and keeps the file names.
    #[test]
    fn tar_without_terminal_does_not_prompt() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = test_output_dir().join("cli_no_prompt_cleaned");
        let tar = test_output_dir().join("cli_no_prompt.tar.gz");
        let _ = std::fs::remove_dir_all(&target);

        let mut command = Command::new(env!("CARGO_BIN_EXE_alc-ng"));
        command
            .arg(root.join("tests/spaces"))
            .arg(&target)
            .arg("--tar")
            .arg(&tar)
            .arg("--skip-watermark")
            .stdin(Stdio::null());

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || sender.send(command.output()));
        let output = receiver
            .recv_timeout(Duration::from_secs(300))
            .expect("alc-ng did not finish, is it waiting for input?")
            .unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(target.join("main paper.tex").is_file());
        assert!(!target.join("main_paper.tex").exists());

        std::fs::remove_file(&tar).unwrap();
    }
}
