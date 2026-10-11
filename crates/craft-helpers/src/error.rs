//! Crate error type: every failure is typed and actionable, never a panic.

use thiserror::Error;

/// Cap carried into error and collection-error messages.
pub const MESSAGE_CHARS: usize = 200;

#[derive(Debug, Clone, Error)]
pub enum HelperError {
    #[error("GH_TOKEN is missing or empty; export it before querying GitHub")]
    MissingToken,
    #[error("GitHub API request failed: {0}")]
    Http(String),
    #[error("rate limited by GitHub (HTTP {status}); retry-after: {retry_after:?}")]
    RateLimited {
        status: u16,
        retry_after: Option<String>,
    },
    #[error("GitHub response is not valid JSON: {0}")]
    MalformedJson(String),
    #[error("GitHub response exceeds the {0}-byte cap")]
    TooLarge(usize),
    #[error("invalid --since {0:?}: expected <n><s|m|h|d>, at most 366 days")]
    InvalidWindow(String),
    #[error("invalid --min-lines {0}: expected 2..=200")]
    InvalidMinLines(usize),
    #[error("invalid argument {name}: {value}")]
    InvalidArg { name: String, value: String },
    #[error("duplicate scan failed: {0}")]
    Scan(String),
    #[error("io failed: {0}")]
    Io(String),
    #[error("json output failed: {0}")]
    Output(String),
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
