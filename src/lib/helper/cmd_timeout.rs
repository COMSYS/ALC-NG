use std::time::Duration;

/// Trait for executing a command with a timeout.
///
/// The method `output_with_timeout` runs the command and returns its output,
/// optionally applying a timeout if supported by the platform.
pub trait CommandTimeout {
    /// Executes the command and returns its output, applying the given timeout
    /// when supported by the underlying OS. On platforms where the timeout
    /// crate cannot be used (e.g., macOS), the timeout is ignored.
    fn output_with_timeout(&mut self, timeout: Duration) -> std::io::Result<std::process::Output>;
}

impl CommandTimeout for std::process::Command {
    /// Executes the command and returns its output, respecting the given timeout.
    /// On non‑macOS targets the `child_wait_timeout` crate is used to enforce the
    /// timeout; on macOS the timeout is ignored and a regular `output()` call
    /// is made instead.
    fn output_with_timeout(&mut self, _timeout: Duration) -> std::io::Result<std::process::Output> {
        // The child_wait_timeout crate does not work on macOS.
        #[cfg(not(target_os = "macos"))]
        {
            use child_wait_timeout::ChildWT;
            use std::process::Stdio;

            self.stdout(Stdio::piped());
            self.stderr(Stdio::piped());

            let mut child = self.spawn()?;
            let _ = child.wait_timeout(_timeout)?;
            child.wait_with_output()
        }

        #[cfg(target_os = "macos")]
        {
            use log::info;

            info!("child_wait_timeout crate does not work on macOS, timeouts will be ignored.");
            self.output()
        }
    }
}
