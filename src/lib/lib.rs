pub mod arxiv;
pub mod cleaner;
// The compare module renders PDFs via the native pdfium dynamic library and
// cannot be built for wasm targets.
#[cfg(not(target_family = "wasm"))]
pub mod compare;
#[cfg(not(target_family = "wasm"))]
pub mod exif_tool;
// The C ABI surface for testing the wasm cdylib.
#[cfg(target_family = "wasm")]
pub mod ffi;
pub mod helper;
pub mod parsing;
// The compilation spinner wraps subprocess output and is only used by the
// (skipped) LaTeX compile step on wasm targets.
#[cfg(not(target_family = "wasm"))]
pub mod progress;
