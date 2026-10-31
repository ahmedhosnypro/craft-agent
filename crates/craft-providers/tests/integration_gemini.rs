//! Live Gemini generateContent corpus against a real endpoint, driven by
//! the repo-root `.env` (`TEST_BYOK_GEMINI_*`); runs via
//! `just integration_gemini`.

#[path = "byok_env.rs"]
mod byok_env;

use craft_core::error::ProviderError;
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest, StopReason, ToolChoice, ToolSpec};
use craft_providers::gemini::GeminiProvider;
use futures::StreamExt;

use crate::byok_env::live_turn;

type TestError = Box<dyn std::error::Error + Send + Sync>;

// Bounds every live turn; a stalled host is a broken corpus, not a hang.
const LIVE_BOUND: std::time::Duration = std::time::Duration::from_secs(120);

// Not a credential: the row proves a wrong key is rejected.
const WRONG_KEY: &str = "wrong-key-never-live";

// The minimal request shape shared by the text and wrong-key rows.
fn minimal_request(system: &str, ask: &str) -> ModelRequest {
    ModelRequest {
        system: Some(String::from(system)),
        messages: vec![std::sync::Arc::new(ChatMessage::User {
            text: String::from(ask),
            images: Vec::new(),
        })],
        tools: Vec::new(),
        max_tokens: 512,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    }
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_GEMINI_* in .env"]
async fn integration_gemini_text_turn() -> Result<(), TestError> {
    let keys = crate::require_gemini_keys!("gemini text");
    let provider = GeminiProvider::new(&keys.base_url, &keys.model, &keys.api_key)?;
    let request = minimal_request(
        "You are a test agent. Answer briefly.",
        "Reply with one short sentence confirming you are running.",
    );
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

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_GEMINI_* in .env"]
async fn integration_gemini_forced_tool_call() -> Result<(), TestError> {
    let keys = crate::require_gemini_keys!("gemini tools");
    let provider = GeminiProvider::new(&keys.base_url, &keys.model, &keys.api_key)?;
    let request = ModelRequest {
        system: Some(String::from(
            "You are a test agent. Use the tool you are given.",
        )),
        messages: vec![std::sync::Arc::new(ChatMessage::User {
            text: String::from("Call the probe tool once with the value 'pong'."),
            images: Vec::new(),
        })],
        tools: vec![ToolSpec {
            name: String::from("probe"),
            description: String::from("Records the given value."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"],
            }),
        }],
        max_tokens: 512,
        tool_choice: Some(ToolChoice::Any),
        sampling: None,
        thinking: None,
        thinking_level: None,
    };
    let turn = tokio::time::timeout(LIVE_BOUND, live_turn(&provider, request))
        .await
        .map_err(|_| String::from("the tool-call turn exceeded the 120s bound"))??;

    assert_eq!(
        turn.stop_reason,
        StopReason::ToolUse,
        "the forced choice must end on a tool call"
    );
    let call = turn
        .tool_calls
        .first()
        .ok_or_else(|| String::from("the forced choice must carry the call"))?;
    assert_eq!(call.name, "probe", "the declared tool must be called");
    let arguments: serde_json::Value = serde_json::from_str(&call.arguments)
        .map_err(|_| String::from("the streamed arguments must assemble into JSON"))?;
    let value = arguments
        .get("value")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| String::from("the call must carry a string value"))?;
    assert!(!value.is_empty(), "the prompted value must ride the call");
    println!(
        "live forced tool call ok: name {}, value {value}",
        call.name
    );
    Ok(())
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_GEMINI_* in .env"]
async fn integration_gemini_wrong_key() -> Result<(), TestError> {
    let keys = crate::require_gemini_keys!("gemini auth");
    assert!(
        keys.api_key != WRONG_KEY,
        "the configured API key collides with the wrong-key constant"
    );
    let provider = GeminiProvider::new(&keys.base_url, &keys.model, WRONG_KEY)?;
    let request = minimal_request("You are a test agent.", "ping");
    let err = tokio::time::timeout(LIVE_BOUND, surface_error(&provider, request))
        .await
        .map_err(|_| String::from("the wrong-key turn must fail fast, never hang"))??;
    assert!(
        matches!(err, ProviderError::Auth { .. }),
        "a wrong key must surface the typed Auth error, got {err:?}"
    );
    Ok(())
}

// Surfaces the first typed error, whether it arrives at the handshake
// or as the first stream item.
async fn surface_error(
    provider: &GeminiProvider,
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
