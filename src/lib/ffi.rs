//! Minimal C ABI surface for testing the cleaner from a web page (or other
//! non-Rust hosts) through the cdylib wasm build.
//!
//! Strings crossing the boundary are NUL-terminated UTF-8. Input buffers are
//! allocated by [`alc_ng_alloc`] from the host and freed again by the host.

use std::ffi::CString;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::sync::Arc;

use crate::cleaner::config::CleanerConfig;
use crate::cleaner::submission::parsed_file::ContentStripper;
use crate::cleaner::submission::Submission;
use crate::parsing;

/// Returns the crate version as a static, NUL-terminated string.
#[unsafe(no_mangle)]
pub extern "C" fn alc_ng_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), " (wasm)\0")
        .as_ptr()
        .cast()
}

/// Runs the module's constructors, which a cdylib never executes on its own.
/// This sets up wasi-libc's preopens and environment; hosts must call it once
/// after instantiation, before [`alc_ng_run`].
///
/// # Safety
/// Must be called at most once per instantiated module; the constructors
/// are not idempotent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn alc_ng_init() {
    unsafe extern "C" {
        fn __wasm_call_ctors();
    }
    unsafe { __wasm_call_ctors() };
}

/// Allocates `len` bytes that the host can write input data into.
///
/// The buffer lives in wasm memory and must be freed again with
/// [`alc_ng_free_buf`], passing the same `len`.
#[unsafe(no_mangle)]
pub extern "C" fn alc_ng_alloc(len: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(len.max(1), 1).expect("invalid layout");
    unsafe { std::alloc::alloc(layout) }
}

/// Frees a buffer previously returned by [`alc_ng_alloc`].
///
/// # Safety
/// `buf` must have been returned by [`alc_ng_alloc`] and `len` must be the
/// same length passed there. The buffer must not have been freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn alc_ng_free_buf(buf: *mut u8, len: usize) {
    if buf.is_null() {
        return;
    }
    let layout = std::alloc::Layout::from_size_align(len.max(1), 1).expect("invalid layout");
    unsafe { std::alloc::dealloc(buf, layout) };
}

/// Cleans a LaTeX source buffer in memory.
///
/// No compilation is performed and the filesystem is never touched. Returns a
/// NUL-terminated JSON string of the form `{"cleaned": "..."}` or
/// `{"error": "..."}`, which must be freed with [`alc_ng_free_string`].
///
/// # Safety
/// `input` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn alc_ng_clean_tex(input: *const u8, len: usize) -> *mut c_char {
    let bytes = unsafe { std::slice::from_raw_parts(input, len) };
    let result = clean_tex(bytes);
    CString::new(result)
        .expect("result contains no NUL")
        .into_raw()
}

/// Frees a string previously returned by [`alc_ng_clean_tex`].
///
/// # Safety
/// `s` must have been returned by [`alc_ng_clean_tex`] and must not have been
/// freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn alc_ng_free_string(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    unsafe { drop(CString::from_raw(s)) };
}

/// The JSON request accepted by [`alc_ng_run`].
#[derive(serde::Deserialize)]
struct RunRequest {
    /// Submission folder to clean (in the host filesystem).
    input: PathBuf,
    /// Folder the cleaned submission is written to.
    target: PathBuf,
    /// LaTeX compiler command; informational on wasm, which never compiles.
    #[serde(default = "default_latex_cmd")]
    latex_cmd: String,
    #[serde(default)]
    latex_args: Vec<String>,
    /// Maximum log level written to stderr:
    /// `"off"`, `"error"`, `"warn"`, `"info"`, `"debug"` or `"trace"`.
    #[serde(default = "default_log_level")]
    log_level: String,
    /// All [`CleanerConfig`] fields (keep_bib, tar, ...) inline.
    #[serde(flatten)]
    config: CleanerConfig,
}

fn default_latex_cmd() -> String {
    "latexmk".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Minimal [`log`] backend writing to stderr, which wasm hosts forward to
/// their console so the pipeline's progress is visible on the test page.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        eprintln!(
            "[{:.5} {}] {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {}
}

static STDERR_LOG: StderrLog = StderrLog;

/// Runs the full cleaning pipeline over a submission folder in the host's
/// filesystem, behaving like the native CLI (minus the interactive prompt
/// and the compare step, which is native-only).
///
/// `input` must point to `len` readable bytes of a UTF-8 JSON object:
/// `{"input": "/path/submission", "target": "/path/out", "fls_provided":
/// "/path/main.fls", ...}` where the remaining keys are the
/// [`CleanerConfig`] flags. Wasm builds cannot spawn a LaTeX compiler, so
/// the host compiles the submission itself (e.g. with the in-browser
/// engine), records the used files into a `.fls` file and passes it as
/// `fls_provided`.
///
/// Returns a NUL-terminated JSON string `{"ok": true, "target": "..."}`
/// or `{"error": "..."}`, which must be freed with [`alc_ng_free_string`].
///
/// # Safety
/// `input` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn alc_ng_run(input: *const u8, len: usize) -> *mut c_char {
    let bytes = unsafe { std::slice::from_raw_parts(input, len) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_pipeline(bytes)));
    let json = match result {
        Ok(json) => json,
        Err(_) => error_json("The cleaning pipeline panicked. See the host console for details."),
    };
    CString::new(json)
        .expect("result contains no NUL")
        .into_raw()
}

fn run_pipeline(bytes: &[u8]) -> String {
    let req: RunRequest = match serde_json::from_slice(bytes) {
        Ok(req) => req,
        Err(err) => return error_json(&format!("Invalid run configuration: {err}")),
    };

    // Route the pipeline's `log` output to stderr (once per process).
    let level = match req.log_level.as_str() {
        "off" => log::LevelFilter::Off,
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    };
    if log::set_logger(&STDERR_LOG).is_ok() {
        log::set_max_level(level);
    }

    // Panic messages must reach the page: stdout/stderr are forwarded by
    // the host, but the default hook output would be the only copy — make
    // it an explicit line. On panic=abort builds this precedes the trap.
    static HOOK: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    HOOK.get_or_init(|| {
        std::panic::set_hook(Box::new(|info| {
            eprintln!("[PANIC  ] {info}");
        }));
    });

    // Behave like the native CLI when the target exists: since there is no
    // interactive prompt in a browser, always replace it.
    if req.target != req.input && req.target.exists() {
        if let Err(err) = std::fs::remove_dir_all(&req.target) {
            return error_json(&format!(
                "Could not replace the existing target path: {err}"
            ));
        }
    }

    let run_result = Submission::new(
        Arc::new(req.config),
        &req.input,
        &req.target,
        &req.latex_cmd,
        &req.latex_args,
    )
    .and_then(|mut submission| submission.run());

    match run_result {
        Ok(()) => format!(
            "{{\"ok\":true,\"target\":{}}}",
            serde_json::to_string(&req.target.display().to_string()).unwrap()
        ),
        Err(err) => error_json(&format!("{err:#}")),
    }
}

fn clean_tex(bytes: &[u8]) -> String {
    let Some(tree) = parsing::parse(bytes) else {
        return error_json("Could not parse the input as LaTeX");
    };

    let (stripped, stats) = match ContentStripper::clean(
        bytes,
        tree.root_node(),
        "input.tex",
        Arc::new(CleanerConfig::default()),
    ) {
        Ok(v) => v,
        Err(err) => return error_json(&err.to_string()),
    };

    let cleaned = match stripped.map(String::from_utf8) {
        Some(Ok(cleaned)) => cleaned,
        Some(Err(_)) => return error_json("The cleaned output is not valid UTF-8"),
        None => String::new(),
    };

    format!(
        "{{\"cleaned\":{},\"grammar_errors\":{}}}",
        serde_json::to_string(&cleaned).unwrap(),
        stats.grammar_errors.len(),
    )
}

fn error_json(message: &str) -> String {
    format!("{{\"error\":{}}}", serde_json::to_string(message).unwrap())
}
