//! Crate error type: every failure is typed and actionable, never a panic.

use thiserror::Error;

/// Cap carried into error and collection-error messages.
pub const MESSAGE_CHARS: usize = 200;

#[derive(Debug, Error)]
pub enum DigestError {
    #[error("gh collection failed: {0}")]
    Gh(String),
    #[error("gh output is not a valid PR list: {0}")]
    GhOutput(String),
    #[error("jscpd report is invalid: {0}")]
    Jscpd(String),
    #[error("jscpd report exceeds the {0}-byte cap")]
    JscpdTooLarge(usize),
    #[error("window is invalid: {0}")]
    Window(String),
    #[error("could not read the digest prompt at `{path}`: {message}")]
    Prompt { path: String, message: String },
    #[error("digest model turn failed ({category}): {detail}")]
    Turn { category: String, detail: String },
    #[error("bundle write failed: {0}")]
    Bundle(String),
    #[error("io failed: {0}")]
    Io(String),
}

/// Bound a hostile message to MESSAGE_CHARS on a char boundary.
pub fn bounded(message: &str) -> String {
    truncate_chars(message, MESSAGE_CHARS)
}

/// Truncate to at most `cap` chars, never splitting a char.
pub fn truncate_chars(text: &str, cap: usize) -> String {
    match text.char_indices().nth(cap) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}
