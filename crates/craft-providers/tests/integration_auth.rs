//! Live bad-key auth corpus for the provider seam, driven by the
//! repo-root `.env` (`TEST_BYOK_*`); runs via `just integration_auth`.

#[path = "byok_env.rs"]
mod byok_env;

use std::error::Error;
use std::time::Duration;

use craft_core::error::ProviderError;
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest};
use craft_providers::openai::OpenAiProvider;
use futures::StreamExt;

type TestError = Box<dyn Error + Send + Sync>;

// Bounds the wrong-key turn; covers the full retry ladder without hanging.
const LIVE_BOUND: Duration = Duration::from_secs(120);
// Not a credential: the row proves a wrong key is rejected.
const WRONG_KEY: &str = "wrong-key-never-live";

// Surfaces the first typed error, whether it arrives at the handshake
// or as the first stream item.
async fn surface_error(
    provider: &OpenAiProvider,
    request: ModelRequest,
) -> Result<ProviderError, TestError> {
    let mut stream = match provider.stream(request).await {
        Ok(stream) => stream,
        Err(err) => return Ok(err),
    };
    match stream.next().await {
        Some(Err(err)) => Ok(err),
        Some(Ok(_)) => Err(String::from("the wrong-key row must fail, not stream").into()),
        None => Err(String::from("the wrong-key row must fail, not close cleanly").into()),
    }
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn integration_auth() -> Result<(), TestError> {
    let keys = crate::require_keys!("auth");
    assert!(
        keys.api_key != WRONG_KEY,
        "the configured API key collides with the wrong-key constant"
    );
    let provider = OpenAiProvider::new(&keys.base_url, &keys.model, WRONG_KEY)?;
    let request = ModelRequest {
        system: Some(String::from("You are a test agent.")),
        messages: vec![ChatMessage::User {
            text: String::from("ping"),
            images: Vec::new(),
        }],
        tools: Vec::new(),
        max_tokens: 16,
        tool_choice: None,
        sampling: None,
    };
    let err = tokio::time::timeout(LIVE_BOUND, surface_error(&provider, request))
        .await
        .map_err(|_| String::from("the wrong-key turn must fail fast, never hang"))??;
    assert!(
        matches!(err, ProviderError::Auth { .. }),
        "a wrong key must surface the typed Auth error, got {err:?}"
    );
    Ok(())
}
