/// Check if a byte is a newline character (handles both LF and CRLF).
/// Returns true for `\n` (Unix) and `\r` (Windows carriage return).
pub fn is_newline(byte: u8) -> bool {
    byte == b'\n' || byte == b'\r'
}
