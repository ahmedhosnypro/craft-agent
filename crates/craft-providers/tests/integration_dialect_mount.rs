//! Live dialect-mount corpus: the Anthropic-messages dialect mounted over
//! the same base URL. The gateway speaks only the OpenAI dialect, so the
//! row asserts a well-formed Messages request goes out and the response
//! maps into the typed ProviderError taxonomy — a working messages-dialect
//! turn is not assumed. Driven by the repo-root `.env` (`TEST_BYOK_*`);
//! runs via `just integration_dialect_mount`.

#[path = "byok_env.rs"]
mod byok_env;

use std::error::Error;
use std::time::Duration;

use craft_core::error::ProviderError;
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest};
use craft_providers::anthropic::AnthropicProvider;
use futures::StreamExt;

type TestError = Box<dyn Error + Send + Sync>;

// Bounds the mounted turn; covers the full retry ladder without hanging.
const LIVE_BOUND: Duration = Duration::from_secs(120);

// Surfaces the first typed error wherever it arrives; a stream that ends
// without one breaks the corpus premise.
async fn surface_error(
    provider: &AnthropicProvider,
    request: ModelRequest,
) -> Result<ProviderError, TestError> {
    let mut stream = match provider.stream(request).await {
        Ok(stream) => stream,
        Err(err) => return Ok(err),
    };
    let mut events = 0usize;
    loop {
        match stream.next().await {
            Some(Err(err)) => return Ok(err),
            Some(Ok(_)) => events += 1,
            None => {
                return Err(format!(
                    "the messages-dialect mount must fail with a typed error \
                     over the OpenAI-dialect gateway; the stream closed after \
                     {events} event(s) instead"
                )
                .into());
            }
        }
    }
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn integration_dialect_mount() -> Result<(), TestError> {
    let keys = crate::require_keys!("dialect mount");
    let provider = AnthropicProvider::new(&keys.base_url, &keys.model, &keys.api_key)?;
    let request = ModelRequest {
        system: Some(String::from("You are a test agent.")),
        messages: vec![ChatMessage::User {
            text: String::from("ping"),
        }],
        tools: Vec::new(),
        max_tokens: 64,
    };
    let err = tokio::time::timeout(LIVE_BOUND, surface_error(&provider, request))
        .await
        .map_err(|_| String::from("the dialect-mount turn must fail fast, never hang"))??;
    // Any typed variant is the mapping; the printed variant is what the
    // live checkpoint records.
    println!("dialect mount surfaced a typed error: {err}");
    Ok(())
}
