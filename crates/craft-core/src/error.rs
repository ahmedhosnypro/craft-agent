//! Closed provider error taxonomy.

use std::time::Duration;

/// Retry ladder shared by every adapter: at most 6 attempts, exponential
/// backoff starting at 2s and capped at 30s; a Retry-After header
/// overrides the ladder; 5 consecutive 429s surface as RateLimit.
pub const RETRY_MAX_ATTEMPTS: usize = 6;
pub const RETRY_BACKOFF_INITIAL_SECS: u64 = 2;
pub const RETRY_BACKOFF_CAP_SECS: u64 = 30;
pub const RATE_LIMIT_STREAK: usize = 5;

/// Every provider failure mode, closed set. Adapters refine body-driven
/// cases (for example a context-window rejection inside a 400) before
/// surfacing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("authentication failed: {detail}")]
    Auth { detail: String },
    #[error("rate limited; retry after: {retry_after:?}")]
    RateLimit { retry_after: Option<Duration> },
    #[error("provider quota exhausted: {detail}")]
    QuotaExhausted { detail: String },
    #[error("transient provider error: {detail}")]
    Transient { detail: String },
    #[error("request payload too large: {detail}")]
    PayloadTooLarge { detail: String },
    #[error("request exceeds the model context window: {detail}")]
    ContextWindow { detail: String },
    #[error("provider rejected the request: {detail}")]
    InvalidRequest { detail: String },
    #[error("transport error: {detail}")]
    Transport { detail: String },
    #[error("request timed out: {detail}")]
    Timeout { detail: String },
    #[error("provider stream stalled within the idle window")]
    IdleTimeout,
    #[error("provider stream closed before the turn finished")]
    StreamClosed,
    #[error("provider returned an empty response")]
    EmptyResponse,
}

impl ProviderError {
    /// Status-code classification shared by every adapter; body-driven
    /// refinements happen before this is called.
    pub fn from_http_status(status: reqwest::StatusCode) -> Self {
        if status.as_u16() == 401 || status.as_u16() == 403 {
            Self::Auth {
                detail: status.to_string(),
            }
        } else if status.as_u16() == 429 {
            Self::RateLimit { retry_after: None }
        } else if status.as_u16() == 413 {
            Self::PayloadTooLarge {
                detail: status.to_string(),
            }
        } else if status.is_server_error() || status.as_u16() == 408 {
            Self::Transient {
                detail: status.to_string(),
            }
        } else if status.is_client_error() {
            Self::InvalidRequest {
                detail: status.to_string(),
            }
        } else {
            Self::Transport {
                detail: format!("unexpected HTTP status {status}"),
            }
        }
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(err: reqwest::Error) -> Self {
        if let Some(status) = err.status() {
            return Self::from_http_status(status);
        }
        let detail = err.to_string();
        if err.is_builder() {
            Self::InvalidRequest { detail }
        } else if err.is_timeout() {
            Self::Transient { detail }
        } else {
            Self::Transport { detail }
        }
    }
}

/// Retry-After in the delta-seconds form only; HTTP-date and junk map to
/// None so the caller falls back to its own ladder.
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    let seconds = value.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds))
}
