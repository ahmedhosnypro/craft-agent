//! Shared HTTP transport for the adapters: the connect budget, the total
//! request deadline behind `CRAFT_AGENT_REQUEST_TIMEOUT_SECS`, and the
//! reqwest classification the retry ladder consumes.

use std::time::Duration;

use craft_core::error::ProviderError;

/// TCP connect budget shared by every adapter; the total deadline below
/// covers the rest of the request.
pub const CONNECT_TIMEOUT_SECS: u64 = 15;

/// Total request deadline when the env knob is unset or empty; the
/// deadline spans connect, headers, and the streaming body read.
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 300;

/// The env knob overriding the total request deadline, in seconds.
pub const REQUEST_TIMEOUT_ENV: &str = "CRAFT_AGENT_REQUEST_TIMEOUT_SECS";

/// One env value into the total deadline: unset or empty keeps the
/// default, and anything unparseable or non-positive is a typed error
/// naming the knob.
pub fn request_timeout(value: Option<&str>) -> Result<Duration, ProviderError> {
    let Some(raw) = value.filter(|raw| !raw.is_empty()) else {
        return Ok(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS));
    };
    let secs = raw.trim().parse::<u64>().map_err(|_| invalid(raw))?;
    if secs == 0 {
        return Err(invalid(raw));
    }
    Ok(Duration::from_secs(secs))
}

/// The total deadline from the live process environment.
pub(crate) fn request_timeout_from_env() -> Result<Duration, ProviderError> {
    match std::env::var(REQUEST_TIMEOUT_ENV) {
        Ok(value) => request_timeout(Some(&value)),
        Err(std::env::VarError::NotPresent) => request_timeout(None),
        Err(err) => Err(invalid(&err.to_string())),
    }
}

/// The client every adapter shares: a bounded connect and a total
/// request deadline covering headers and streaming body reads.
pub(crate) fn client() -> Result<reqwest::Client, ProviderError> {
    client_with_timeout(request_timeout_from_env()?)
}

/// The same client over an explicit total deadline.
pub(crate) fn client_with_timeout(total: Duration) -> Result<reqwest::Client, ProviderError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .timeout(total)
        .build()
        .map_err(ProviderError::from)
}

/// Ladder classification: a reqwest timeout surfaces as the retryable
/// Timeout row; everything else follows the shared taxonomy.
pub(crate) fn classify(err: reqwest::Error) -> ProviderError {
    if err.is_timeout() {
        return ProviderError::Timeout {
            detail: err.to_string(),
        };
    }
    ProviderError::from(err)
}

fn invalid(raw: &str) -> ProviderError {
    ProviderError::InvalidRequest {
        detail: format!(
            "{REQUEST_TIMEOUT_ENV} must be a positive whole number of seconds; got `{raw}`"
        ),
    }
}
