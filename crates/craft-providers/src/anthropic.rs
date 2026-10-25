//! Anthropic Messages adapter: request encoding, named-event SSE decode,
//! error mapping, and the shared retry ladder, over the neutral
//! provider seam.

mod caching;
mod ceiling;
mod chunk;
mod ids;
mod images;
mod lane;
mod request;

use std::time::Duration;

use craft_core::error::{ProviderError, parse_retry_after};
use craft_core::provider::{ModelEventStream, Provider};
use craft_core::types::ModelRequest;
use serde_json::Value;

use chunk::WireError;
use lane::lane_stream;

use crate::retry;
use crate::transport;

/// Production default; the fixed Messages host. The endpoint is not
/// overridable in the wild, but the constructor keeps the seam so the
/// corpus mounts a loopback lane.
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// The version header every request carries, per the versioning docs.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// How the key rides on the wire: the documented `x-api-key` header, or
/// a `Bearer` token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthScheme {
    ApiKey,
    Bearer,
}

/// The Messages dialect mounted over one base URL.
pub struct AnthropicProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    retry_patience: Duration,
    auth_scheme: AuthScheme,
    sse: craft_core::sse::SseOptions,
}

impl AnthropicProvider {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Result<Self, ProviderError> {
        let client = transport::client()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
            api_key: api_key.to_owned(),
            retry_patience: craft_core::error::RETRY_PATIENCE_DEFAULT,
            auth_scheme: AuthScheme::ApiKey,
            sse: craft_core::sse::SseOptions::default(),
        })
    }

    /// Overrides how the key rides; no client rebuild needed.
    pub fn with_auth_scheme(mut self, scheme: AuthScheme) -> Self {
        self.auth_scheme = scheme;
        self
    }

    /// Remounts the lane over the shared transient-patience window.
    pub fn with_retry_patience(mut self, patience: Duration) -> Self {
        self.retry_patience = patience;
        self
    }

    /// Remounts the lane over an explicit total request deadline; the
    /// constructor's deadline comes from the shared env knob.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Result<Self, ProviderError> {
        self.client = transport::client_with_timeout(timeout)?;
        Ok(self)
    }

    /// Sends one request on the shared ladder, mounted over this
    /// dialect's URL, headers, and payload.
    async fn connect(&self, request: &ModelRequest) -> Result<reqwest::Response, ProviderError> {
        let body = request::encode(request, &self.model)?;
        let payload =
            serde_json::to_string(&body).map_err(|err| ProviderError::InvalidRequest {
                detail: err.to_string(),
            })?;
        let url = format!("{}/v1/messages", self.base_url);
        retry::climb(
            &self.client,
            |client| {
                let request = client
                    .post(&url)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(payload.clone());
                match self.auth_scheme {
                    AuthScheme::ApiKey => request.header("x-api-key", &self.api_key),
                    AuthScheme::Bearer => request.bearer_auth(&self.api_key),
                }
            },
            http_failure,
            self.retry_patience,
        )
        .await
    }
}

impl Provider for AnthropicProvider {
    async fn stream(&self, request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        let response = self.connect(&request).await?;
        Ok(lane_stream(response, self.sse))
    }
}

/// Non-2xx responses: the live Retry-After header and the JSON error
/// body refine the status classification before the taxonomy closes.
async fn http_failure(response: reqwest::Response) -> ProviderError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after);
    let body = retry::read_capped(response, retry::ERROR_BODY_CAP_BYTES).await;
    let message = error_body_message(&body);
    let detail = match message.as_deref() {
        Some(message) => format!("{status}: {message}"),
        None => status.to_string(),
    };
    classify_http_failure(status, message.as_deref(), retry_after, detail)
}

/// The `error.message` field from a JSON error body, if present.
fn error_body_message(body: &str) -> Option<String> {
    let envelope = serde_json::from_str::<Value>(body).ok()?;
    envelope
        .get("error")?
        .get("message")?
        .as_str()
        .map(str::to_owned)
}

/// Status-specific rows; anything else falls through to body-driven
/// classification.
fn classify_http_failure(
    status: reqwest::StatusCode,
    message: Option<&str>,
    retry_after: Option<Duration>,
    detail: String,
) -> ProviderError {
    match status.as_u16() {
        401 | 403 => ProviderError::Auth { detail },
        429 => ProviderError::RateLimit { retry_after },
        413 => ProviderError::PayloadTooLarge { detail },
        _ => classify_by_body(status, message, detail),
    }
}

/// Body-driven refinement: a long-prompt rejection inside a 400 is a
/// context-window failure, not a generic rejection.
fn classify_by_body(
    status: reqwest::StatusCode,
    message: Option<&str>,
    detail: String,
) -> ProviderError {
    if is_context_window(message) {
        ProviderError::ContextWindow { detail }
    } else if status.is_server_error() || status.as_u16() == 408 {
        ProviderError::Transient { detail }
    } else if status.is_client_error() {
        ProviderError::InvalidRequest { detail }
    } else {
        ProviderError::from_http_status(status)
    }
}

/// Context-window rejections are named in the body text.
fn is_context_window(message: Option<&str>) -> bool {
    message.is_some_and(|message| {
        message.contains("context window") || message.contains("prompt is too long")
    })
}

/// Mid-stream error events map onto the same taxonomy as HTTP rows.
fn map_error_envelope(error: WireError) -> ProviderError {
    let detail = error
        .message
        .clone()
        .unwrap_or_else(|| String::from("provider error mid-stream"));
    match error.kind.as_deref() {
        Some("rate_limit_error") => ProviderError::RateLimit { retry_after: None },
        Some("overloaded_error") | Some("api_error") | Some("timeout_error") => {
            ProviderError::Transient { detail }
        }
        Some("invalid_request_error") | Some("not_found_error") => {
            ProviderError::InvalidRequest { detail }
        }
        Some("authentication_error") | Some("permission_error") => ProviderError::Auth { detail },
        kind => ProviderError::Transport {
            detail: format!("{}: {detail}", kind.unwrap_or("unknown error")),
        },
    }
}
