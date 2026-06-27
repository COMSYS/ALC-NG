use indicatif::{ProgressBar, ProgressStyle};
use std::{io::IsTerminal, time::Duration};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// A lightweight spinner shown while LaTeX files compile.
///
/// Prints to stderr so it doesn't interfere with log output on stdout.
/// Automatically disabled when stderr is not a terminal.
pub struct CompilationSpinner {
    pb: Option<ProgressBar>,
    current: usize,
    total: usize,
}

impl CompilationSpinner {
    /// Creates a new spinner for `total` files.
    ///
    /// If stderr is not a terminal, the spinner is a no-op.
    pub fn new(total: usize) -> Self {
        let pb = if std::io::stderr().is_terminal() && total > 0 {
            let pb = ProgressBar::no_length();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .tick_strings(SPINNER_FRAMES)
                    .template("{spinner} Compiling {msg}")
                    .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_millis(80));
            Some(pb)
        } else {
            None
        };

        Self {
            pb,
            current: 0,
            total,
        }
    }

    /// Updates the spinner to show the current file being compiled.
    pub fn update(&mut self, filename: &str) {
        self.current += 1;
        if let Some(pb) = &self.pb {
            pb.set_message(format!("{} [{} / {}]", filename, self.current, self.total));
        } else {
            log::info!("Compiling main file (this may take a while): {filename}");
        }
    }

    /// Finishes the spinner and clears it from the terminal.
    pub fn finish(&mut self) {
        if let Some(pb) = &self.pb {
            pb.finish_with_message(format!("done ({} compiled)", self.total));
        }
    }
}
