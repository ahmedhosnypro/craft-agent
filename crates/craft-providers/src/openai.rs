//! OpenAI Chat Completions adapter: request encoding, SSE decode, error
//! mapping, and the shared retry ladder, over the neutral provider seam.

mod chunk;
mod lane;
mod request;

use std::time::Duration;

use craft_core::error::{
    ProviderError, RATE_LIMIT_STREAK, RETRY_BACKOFF_CAP_SECS, RETRY_BACKOFF_INITIAL_SECS,
    RETRY_MAX_ATTEMPTS, parse_retry_after,
};
use craft_core::provider::{ModelEventStream, Provider};
use craft_core::types::ModelRequest;
use serde_json::Value;

use chunk::WireError;
use lane::lane_stream;

use crate::transport;

/// Production default; the base includes the version segment, and a
/// caller may override it for any OpenAI-compatible lane.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Bound on a non-2xx body read; error bodies are classification input,
/// so a hostile server cannot balloon the process.
const ERROR_BODY_CAP_BYTES: usize = 64 * 1024;

/// The Chat Completions dialect mounted over one base URL.
pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    model: String,
    api_key: String,
    sse: craft_core::sse::SseOptions,
}

impl OpenAiProvider {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Result<Self, ProviderError> {
        let client = transport::client()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            model: model.to_owned(),
            api_key: api_key.to_owned(),
            sse: craft_core::sse::SseOptions::default(),
        })
    }

    /// Remounts the lane over an explicit total request deadline; the
    /// constructor's deadline comes from the shared env knob.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Result<Self, ProviderError> {
        self.client = transport::client_with_timeout(timeout)?;
        Ok(self)
    }

    /// Sends one request on the shared ladder: auth and client errors
    /// fail fast, a 429 streak surfaces with the live Retry-After, and
    /// transient, transport, or timeout failures back off — all bounded
    /// by the shared error constants.
    async fn connect(&self, request: &ModelRequest) -> Result<reqwest::Response, ProviderError> {
        let body = request::encode(request, &self.model)?;
        let payload =
            serde_json::to_string(&body).map_err(|err| ProviderError::InvalidRequest {
                detail: err.to_string(),
            })?;
        let url = format!("{}/chat/completions", self.base_url);
        let mut backoff = Duration::from_secs(RETRY_BACKOFF_INITIAL_SECS);
        let mut rate_streak = 0usize;
        let mut last = None;
        for _ in 0..RETRY_MAX_ATTEMPTS {
            let attempt = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(payload.clone())
                .send()
                .await;
            let failure = match attempt {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => http_failure(response).await,
                Err(err) => transport::classify(err),
            };
            last = Some(failure.clone());
            let wait = match &failure {
                ProviderError::RateLimit { retry_after } => {
                    rate_streak += 1;
                    if rate_streak >= RATE_LIMIT_STREAK {
                        break;
                    }
                    let ceiling = Duration::from_secs(RETRY_BACKOFF_CAP_SECS);
                    Some(retry_after.map_or(backoff, |after| after.min(ceiling)))
                }
                ProviderError::Transient { .. }
                | ProviderError::Transport { .. }
                | ProviderError::Timeout { .. } => {
                    rate_streak = 0;
                    Some(backoff)
                }
                _ => None,
            };
            let Some(wait) = wait else {
                return Err(failure);
            };
            tokio::time::sleep(wait).await;
            backoff = ladder_step(backoff);
        }
        Err(last.unwrap_or(ProviderError::Transport {
            detail: String::from("the retry ladder exhausted without an attempt"),
        }))
    }
}

impl Provider for OpenAiProvider {
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
    let body = read_capped(response, ERROR_BODY_CAP_BYTES).await;
    let envelope = serde_json::from_str::<Value>(&body).ok();
    let error = envelope.as_ref().and_then(|value| value.get("error"));
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str);
    let code = error
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str);
    let detail = match message {
        Some(message) => format!("{status}: {message}"),
        None => status.to_string(),
    };
    if status.as_u16() == 401 || status.as_u16() == 403 {
        ProviderError::Auth { detail }
    } else if status.as_u16() == 429 {
        ProviderError::RateLimit { retry_after }
    } else if status.as_u16() == 413 {
        ProviderError::PayloadTooLarge { detail }
    } else if is_context_window(code, message) {
        ProviderError::ContextWindow { detail }
    } else if status.is_server_error() || status.as_u16() == 408 {
        ProviderError::Transient { detail }
    } else if status.is_client_error() {
        ProviderError::InvalidRequest { detail }
    } else {
        ProviderError::from_http_status(status)
    }
}

fn is_context_window(code: Option<&str>, message: Option<&str>) -> bool {
    code.is_some_and(|code| code.contains("context_length"))
        || message.is_some_and(|message| message.contains("context length"))
}

/// Mid-stream error envelopes map onto the same taxonomy as HTTP rows.
fn map_error_envelope(error: WireError) -> ProviderError {
    let detail = error
        .message
        .clone()
        .unwrap_or_else(|| String::from("provider error mid-stream"));
    let kind = error.kind.as_deref();
    let code = error.code.as_deref();
    if kind.is_some_and(|kind| kind.contains("rate_limit")) || code == Some("insufficient_quota") {
        return ProviderError::RateLimit { retry_after: None };
    }
    if is_context_window(code, error.message.as_deref()) {
        return ProviderError::ContextWindow { detail };
    }
    match kind {
        Some("server_error") | Some("service_unavailable") => ProviderError::Transient { detail },
        Some("invalid_request_error") => ProviderError::InvalidRequest { detail },
        Some("authentication_error") | Some("permission_error") => ProviderError::Auth { detail },
        _ => ProviderError::Transport {
            detail: format!("{}: {detail}", kind.unwrap_or("unknown error")),
        },
    }
}

/// Reads at most `cap` bytes of a response body, lossily.
async fn read_capped(mut response: reqwest::Response, cap: usize) -> String {
    let mut raw: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.ok().flatten() {
        if raw.len().saturating_add(chunk.len()) > cap {
            break;
        }
        raw.extend_from_slice(&chunk);
    }
    String::from_utf8_lossy(&raw).into_owned()
}

fn ladder_step(previous: Duration) -> Duration {
    let grown = previous.saturating_mul(2);
    Duration::from_secs(grown.as_secs().min(RETRY_BACKOFF_CAP_SECS))
}
