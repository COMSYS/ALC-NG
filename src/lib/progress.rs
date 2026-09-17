use indicatif::{ProgressBar, ProgressStyle};
use std::{
    borrow::Cow,
    collections::VecDeque,
    io::{BufRead, BufReader, IsTerminal, Read},
    process::{Command, Output},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Number of recent subprocess output lines kept visible under the spinner.
const TAIL_LINES: usize = 7;
/// Width used when the terminal size cannot be determined.
const FALLBACK_WIDTH: usize = 80;
/// Column width of the `"{spinner} Compiling "` prefix rendered before `{msg}`.
const PREFIX_WIDTH: usize = 12;
/// ANSI bright-black ("faint") style for the streamed output lines, so they
/// are visually distinct from alc-ng's own logging.
const FAINT: &str = "\u{1b}[90m";
/// ANSI reset, ending the faint style of a streamed output line.
const RESET: &str = "\u{1b}[0m";

/// Width of the stderr terminal in columns.
fn term_width() -> usize {
    // `size_checked` returns (rows, columns).
    console::Term::stderr()
        .size_checked()
        .map(|(_, w)| w as usize)
        .filter(|w| *w > 0)
        .unwrap_or(FALLBACK_WIDTH)
}

/// Truncates `line` to at most `width` columns.
///
/// A line wider than the terminal would wrap onto several visual lines while
/// indicatif still counts it as one, desynchronizing its line bookkeeping.
fn truncate_to_width(line: &str, width: usize) -> Cow<'_, str> {
    if line.chars().count() <= width {
        Cow::Borrowed(line)
    } else {
        Cow::Owned(line.chars().take(width).collect())
    }
}

struct SpinnerState {
    pb: Option<ProgressBar>,
    label: String,
    tail: VecDeque<String>,
    current: usize,
    finished: bool,
}

/// A spinner shown while LaTeX files compile.
///
/// In addition to the spinner line, the last few lines of the subprocess
/// output are streamed live underneath it, similar to `docker build`.
///
/// The whole block (spinner line + tail) is rendered as a single multi-line
/// message on one [`ProgressBar`]. Each message line is counted correctly by
/// indicatif on re-render, so the block is cleared and redrawn cleanly as the
/// tail grows and shifts.
///
/// Prints to stderr so it doesn't interfere with log output on stdout.
/// Automatically disabled when stderr is not a terminal.
#[derive(Clone)]
pub struct CompilationSpinner {
    tty: bool,
    total: usize,
    state: Arc<Mutex<SpinnerState>>,
}

impl Drop for CompilationSpinner {
    fn drop(&mut self) {
        self.finish();
    }
}

impl CompilationSpinner {
    /// Creates a new spinner for `total` files.
    ///
    /// If stderr is not a terminal, the display is a no-op.
    pub fn new(total: usize) -> Self {
        let tty = std::io::stderr().is_terminal();
        let pb = if tty && total > 0 {
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
            tty,
            total,
            state: Arc::new(Mutex::new(SpinnerState {
                pb,
                label: String::new(),
                tail: VecDeque::new(),
                current: 0,
                finished: false,
            })),
        }
    }

    /// Renders the spinner block as a single multi-line message: the label on
    /// the first line followed by the live output tail.
    fn render(state: &SpinnerState) -> String {
        let mut msg = state.label.clone();
        for line in &state.tail {
            // Faint style on the streamed lines only; the label above stays in
            // the default color so the two are easy to tell apart.
            msg.push('\n');
            msg.push_str(FAINT);
            msg.push_str(line);
            msg.push_str(RESET);
        }
        msg
    }

    /// Marks the start of a new compilation, clearing the live output tail
    /// and updating the spinner to show the current file being compiled.
    pub fn update(&self, filename: &str) {
        let mut state = self.state.lock().unwrap();
        state.current += 1;

        if !self.tty {
            log::info!("Compiling main file (this may take a while): {filename}");
            return;
        }

        state.tail.clear();
        let label = format!("{} [{} / {}]", filename, state.current, self.total);
        state.label =
            truncate_to_width(&label, term_width().saturating_sub(PREFIX_WIDTH)).into_owned();
        let pb = state.pb.clone();
        let msg = Self::render(&state);
        drop(state);
        if let Some(pb) = pb {
            pb.set_message(msg);
        }
    }

    /// Appends one line of subprocess output to the live tail.
    ///
    /// Thread-safe: intended to be called from the stdout/stderr reader
    /// threads spawned by [`CompilationSpinner::run_piped`].
    pub fn push_line(&self, line: &str) {
        if !self.tty {
            return;
        }

        // `read_until` includes the delimiter in the chunk. A trailing newline
        // in a message line desynchronizes indicatif's line bookkeeping (it
        // would be written but not counted as a line), so strip it.
        let line = line.trim_end_matches(['\n', '\r']);
        // Lines longer than the terminal would wrap and desynchronize the line
        // bookkeeping as well, so truncate them.
        let line = truncate_to_width(line, term_width());

        let mut state = self.state.lock().unwrap();
        if state.finished {
            return;
        }

        state.tail.push_back(line.into_owned());
        while state.tail.len() > TAIL_LINES {
            state.tail.pop_front();
        }
        let pb = state.pb.clone();
        let msg = Self::render(&state);
        drop(state);
        if let Some(pb) = pb {
            pb.set_message(msg);
        }
    }

    /// Runs `cmd`, streaming its stdout/stderr into the live tail while
    /// capturing the full output, which is returned once the process exits.
    ///
    /// `cmd` must have both stdout and stderr set to `Stdio::piped()`.
    pub fn run_piped(&self, cmd: &mut Command) -> std::io::Result<Output> {
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let stdout_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

        let mut readers = Vec::new();
        if let Some(stdout) = stdout {
            let spinner = self.clone();
            let buf = Arc::clone(&stdout_buf);
            readers.push(thread::spawn(move || {
                drain_lines(BufReader::new(stdout), &spinner, &buf);
            }));
        }
        if let Some(stderr) = stderr {
            let spinner = self.clone();
            let buf = Arc::clone(&stderr_buf);
            readers.push(thread::spawn(move || {
                drain_lines(BufReader::new(stderr), &spinner, &buf);
            }));
        }

        let status = child.wait()?;
        for reader in readers {
            reader.join().expect("output reader thread panicked");
        }

        Ok(Output {
            status,
            stdout: stdout_buf.lock().unwrap().clone(),
            stderr: stderr_buf.lock().unwrap().clone(),
        })
    }

    /// Finishes the spinner and clears the live output tail.
    pub fn finish(&self) {
        if !self.tty {
            return;
        }

        let mut state = self.state.lock().unwrap();
        if state.finished {
            return;
        }
        state.finished = true;

        let pb = state.pb.take();
        if let Some(pb) = pb {
            pb.finish_with_message(format!("done ({} compiled)", self.total));
        }
    }
}

/// Reads `reader` line by line, forwarding each line to the spinner tail
/// while appending the raw bytes to `buf`.
fn drain_lines<R: Read>(
    mut reader: BufReader<R>,
    spinner: &CompilationSpinner,
    buf: &Mutex<Vec<u8>>,
) {
    let mut line = Vec::new();
    loop {
        // `read_until` appends to the buffer, so it must be reset per line.
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {
                spinner.push_line(&String::from_utf8_lossy(&line));
                buf.lock().unwrap().extend_from_slice(&line);
            }
            Err(_) => break,
        }
    }
}
