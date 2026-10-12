//! The connect ladder shared by the adapters: bounded attempts,
//! exponential backoff clamped by a live Retry-After, and the capped
//! non-2xx body read.

use std::future::Future;
use std::time::Duration;

use craft_core::error::{
    ProviderError, RATE_LIMIT_STREAK, RETRY_BACKOFF_CAP_SECS, RETRY_BACKOFF_INITIAL_SECS,
    RETRY_MAX_ATTEMPTS,
};

use crate::transport;

/// Bound on a non-2xx body read; error bodies are classification input,
/// so a hostile server cannot balloon the process.
pub(crate) const ERROR_BODY_CAP_BYTES: usize = 64 * 1024;

/// One request on the shared ladder: auth and client errors fail fast,
/// a 429 streak surfaces with the live Retry-After, and transient,
/// transport, or timeout failures back off — all bounded by the shared
/// error constants. The builder mounts the dialect's URL, headers, and
/// payload; the classifier refines each non-2xx body.
pub(crate) async fn climb<B, F, Fut>(
    client: &reqwest::Client,
    build: B,
    http_failure: F,
) -> Result<reqwest::Response, ProviderError>
where
    B: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    F: Fn(reqwest::Response) -> Fut,
    Fut: Future<Output = ProviderError>,
{
    let mut backoff = Duration::from_secs(RETRY_BACKOFF_INITIAL_SECS);
    let mut rate_streak = 0usize;
    let mut last = None;
    for round in 1..=RETRY_MAX_ATTEMPTS {
        let failure = match build(client).send().await {
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
        if round == RETRY_MAX_ATTEMPTS {
            return Err(failure);
        }
        tokio::time::sleep(wait).await;
        backoff = ladder_step(backoff);
    }
    Err(last.unwrap_or(ProviderError::Transport {
        detail: String::from("the retry ladder exhausted without an attempt"),
    }))
}

/// Reads at most `cap` bytes of a response body, lossily.
pub(crate) async fn read_capped(mut response: reqwest::Response, cap: usize) -> String {
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
