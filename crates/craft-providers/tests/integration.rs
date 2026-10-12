//! Live completions corpus for the provider seam, driven by the repo-root
//! `.env` (`TEST_BYOK_*`); runs via `just integration`.

#[path = "byok_env.rs"]
mod byok_env;

use std::error::Error;
use std::time::Duration;

use craft_core::events::{FoldedTurn, TurnCaps, TurnFold};
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest, StopReason};
use craft_providers::openai::OpenAiProvider;
use futures::StreamExt;

type TestError = Box<dyn Error + Send + Sync>;

// Bounds every live turn; a stalled gateway is a broken corpus, not a hang.
const LIVE_BOUND: Duration = Duration::from_secs(120);

// Streams one turn end to end, folding events as they arrive; every
// handshake, stream, or fold failure is a loud test failure.
async fn live_turn(
    provider: &OpenAiProvider,
    request: ModelRequest,
) -> Result<FoldedTurn, TestError> {
    let mut stream = provider.stream(request).await?;
    let mut fold = TurnFold::new(TurnCaps::default());
    while let Some(item) = stream.next().await {
        let event = item?;
        fold.push(event)?;
    }
    Ok(fold.finish()?)
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn integration() -> Result<(), TestError> {
    let keys = crate::require_keys!("completions e2e");
    let provider = OpenAiProvider::new(&keys.base_url, &keys.model, &keys.api_key)?;
    let request = ModelRequest {
        system: Some(String::from("You are a test agent. Answer briefly.")),
        messages: vec![ChatMessage::User {
            text: String::from("Reply with one short sentence confirming you are running."),
            images: Vec::new(),
        }],
        tools: Vec::new(),
        max_tokens: 512,
        tool_choice: None,
        sampling: None,
    };
    let turn = tokio::time::timeout(LIVE_BOUND, live_turn(&provider, request))
        .await
        .map_err(|_| String::from("the live turn exceeded the 120s bound"))??;

    assert!(
        !turn.text.trim().is_empty(),
        "the streamed turn must produce coherent text"
    );
    assert!(
        !matches!(
            turn.stop_reason,
            StopReason::ContentFilter | StopReason::Refusal | StopReason::MalformedToolCall
        ),
        "the turn must end on a completion stop reason, got {:?}",
        turn.stop_reason
    );
    assert!(!turn.usage.incomplete, "usage must be reported");
    assert!(
        turn.usage.input_tokens + turn.usage.output_tokens > 0,
        "usage must carry tokens"
    );
    println!(
        "live turn ok: {} text bytes, stop {:?}, usage {}+{}",
        turn.text.len(),
        turn.stop_reason,
        turn.usage.input_tokens,
        turn.usage.output_tokens
    );
    Ok(())
}
