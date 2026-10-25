//! Google Gemini generateContent adapter: request encoding, SSE decode,
//! error mapping, and the shared retry ladder, over the neutral seam.

mod chunk;
pub mod credentials;
mod generation;
mod images;
mod lane;
pub mod registry;
mod request;
pub mod vertex;

use std::time::Duration;

use craft_core::error::{ProviderError, parse_retry_after};
use craft_core::provider::{ModelEventStream, Provider};
use craft_core::types::ModelRequest;

use lane::lane_stream;

use crate::retry;
use crate::transport;

/// Production default host; the adapter appends the version and model
/// path itself, so a lane override stays host-shaped.
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

/// The documented spent-daily-quota markers: the machine-readable code
/// and the anchor sentence of the documented daily-quota message.
const SPENT_QUOTA_CODE: &str = "quota_exceeded";
const SPENT_QUOTA_MESSAGE: &str = "you have exceeded your daily quota";
/// The structured retry hint inside the documented `details` array.
const RETRY_INFO_TYPE: &str = "google.rpc.RetryInfo";

/// The generateContent dialect mounted over one base URL.
pub struct GeminiProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    retry_patience: Duration,
    sse: craft_core::sse::SseOptions,
}

impl GeminiProvider {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Result<Self, ProviderError> {
        let client = transport::client()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
            api_key: api_key.to_owned(),
            retry_patience: craft_core::error::RETRY_PATIENCE_DEFAULT,
            sse: craft_core::sse::SseOptions::default(),
        })
    }

    /// Remounts the lane over an explicit total request deadline; the
    /// constructor's deadline comes from the shared env knob.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Result<Self, ProviderError> {
        self.client = transport::client_with_timeout(timeout)?;
        Ok(self)
    }

    /// Remounts the lane over the shared transient-patience window.
    pub fn with_retry_patience(mut self, patience: Duration) -> Self {
        self.retry_patience = patience;
        self
    }

    /// Sends one streaming request on the shared ladder, mounted over
    /// this dialect's URL, headers, and payload.
    async fn connect(&self, request: &ModelRequest) -> Result<reqwest::Response, ProviderError> {
        let body = request::encode(request, &self.model)?;
        let payload =
            serde_json::to_string(&body).map_err(|err| ProviderError::InvalidRequest {
                detail: err.to_string(),
            })?;
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
            self.base_url, self.model
        );
        retry::climb(
            &self.client,
            |client| {
                client
                    .post(&url)
                    .header("x-goog-api-key", &self.api_key)
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(payload.clone())
            },
            http_failure,
            self.retry_patience,
        )
        .await
    }
}

impl Provider for GeminiProvider {
    async fn stream(&self, request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        let response = self.connect(&request).await?;
        Ok(lane_stream(response, self.sse))
    }
}

/// Non-2xx responses: the live Retry-After header, the error envelope,
/// and the structured RetryInfo delay refine the status classification
/// before the taxonomy closes.
async fn http_failure(response: reqwest::Response) -> ProviderError {
    let status = response.status();
    let header_after = retry_after_header(&response);
    let body = retry::read_capped(response, retry::ERROR_BODY_CAP_BYTES).await;
    let envelope = serde_json::from_str::<serde_json::Value>(&body).ok();
    let error = envelope.as_ref().and_then(|envelope| envelope.get("error"));
    let message = error_message(error);
    let detail = match message {
        Some(message) => format!("{status}: {message}"),
        None => status.to_string(),
    };
    let retry_after = header_after.or_else(|| {
        error
            .and_then(|error| error.get("details"))
            .and_then(serde_json::Value::as_array)
            .and_then(|details| retry_info_delay(details))
    });
    if status.as_u16() == 429
        && is_spent_quota(
            error
                .and_then(|error| error.get("code"))
                .and_then(serde_json::Value::as_str),
            message,
        )
    {
        return ProviderError::QuotaExhausted { detail };
    }
    http_status_error(status, detail, message, retry_after)
}

/// The live Retry-After header, when the response carries a parseable one.
fn retry_after_header(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after)
}

/// The error envelope's human-readable message, if the body carries one.
fn error_message(error: Option<&serde_json::Value>) -> Option<&str> {
    error
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
}

/// The documented daily-quota exhaustion markers: the machine-readable
/// code or the documented message sentence. Without one of them a 429
/// stays a retryable rate limit.
fn is_spent_quota(code: Option<&str>, message: Option<&str>) -> bool {
    code == Some(SPENT_QUOTA_CODE)
        || message.is_some_and(|message| message.to_ascii_lowercase().contains(SPENT_QUOTA_MESSAGE))
}

/// The RetryInfo delay in the documented `details` array, when the body
/// carries one and the value is an actionable non-negative duration.
fn retry_info_delay(details: &[serde_json::Value]) -> Option<Duration> {
    details
        .iter()
        .filter(|detail| {
            detail
                .get("@type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|type_url| type_url.rsplit('/').next() == Some(RETRY_INFO_TYPE))
        })
        .find_map(|detail| {
            detail
                .get("retryDelay")
                .and_then(serde_json::Value::as_str)
                .and_then(parse_google_duration)
        })
}

/// A google.protobuf.Duration JSON value ("42s", "1.212s"); negative and
/// unparseable values are not actionable and map to None.
fn parse_google_duration(value: &str) -> Option<Duration> {
    let body = value.trim().strip_suffix('s')?;
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (body, ""),
    };
    if whole.is_empty() {
        return None;
    }
    let seconds = whole.parse::<u64>().ok()?;
    let mut nanos: u64 = 0;
    for (index, character) in fraction.chars().take(9).enumerate() {
        let digit = u64::from(character.to_digit(10)?);
        let weight = 10u64.checked_pow(8 - index as u32)?;
        nanos = nanos.checked_add(digit.checked_mul(weight)?)?;
    }
    Some(Duration::new(seconds, nanos as u32))
}

/// Maps a non-2xx status onto the taxonomy; the documented token-count
/// rejection rides a plain 400 and is recognized by its message.
fn http_status_error(
    status: reqwest::StatusCode,
    detail: String,
    message: Option<&str>,
    retry_after: Option<Duration>,
) -> ProviderError {
    match status.as_u16() {
        401 | 403 => ProviderError::Auth { detail },
        429 => ProviderError::RateLimit { retry_after },
        413 => ProviderError::PayloadTooLarge { detail },
        _ if is_context_window(message) => ProviderError::ContextWindow { detail },
        408 => ProviderError::Transient { detail },
        _ if status.is_server_error() => ProviderError::Transient { detail },
        _ if status.is_client_error() => ProviderError::InvalidRequest { detail },
        _ => ProviderError::from_http_status(status),
    }
}

/// The documented token-count rejection rides a plain 400.
fn is_context_window(message: Option<&str>) -> bool {
    message.is_some_and(|message| message.contains("context window"))
}

/// A mid-stream error envelope maps onto the same taxonomy as the HTTP
/// rows, keyed on the canonical status name; the structured RetryInfo
/// delay and the spent-quota markers ride the RESOURCE_EXHAUSTED row.
fn map_error_envelope(error: chunk::WireErrorEnvelope) -> ProviderError {
    let detail = error
        .message
        .clone()
        .unwrap_or_else(|| String::from("provider error mid-stream"));
    match error.status.as_deref() {
        Some("UNAUTHENTICATED") | Some("PERMISSION_DENIED") => ProviderError::Auth { detail },
        Some("RESOURCE_EXHAUSTED") => {
            let retry_after = error.details.as_deref().and_then(retry_info_delay);
            let code = error.code.as_ref().and_then(serde_json::Value::as_str);
            if is_spent_quota(code, error.message.as_deref()) {
                ProviderError::QuotaExhausted { detail }
            } else {
                ProviderError::RateLimit { retry_after }
            }
        }
        Some("INTERNAL") | Some("UNAVAILABLE") | Some("DEADLINE_EXCEEDED") | Some("UNKNOWN") => {
            ProviderError::Transient { detail }
        }
        _ if is_context_window(error.message.as_deref()) => ProviderError::ContextWindow { detail },
        Some("INVALID_ARGUMENT") | Some("FAILED_PRECONDITION") | Some("OUT_OF_RANGE") => {
            ProviderError::InvalidRequest { detail }
        }
        _ => ProviderError::Transport {
            detail: format!(
                "{}: {detail}",
                error.status.as_deref().unwrap_or("unknown error")
            ),
        },
    }
}
