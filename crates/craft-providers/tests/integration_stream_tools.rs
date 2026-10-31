//! Live streamed tool-delta assembly corpus, driven by the repo-root
//! `.env` (`TEST_BYOK_*`); runs via `just integration_stream_tools`.

#[path = "byok_env.rs"]
mod byok_env;

use std::cell::Cell;

use craft_core::events::{FoldedTurn, ModelEvent};
use craft_core::types::{ChatMessage, ModelRequest, ToolSpec};
use craft_providers::openai::OpenAiProvider;

use crate::byok_env::live_turn_observed;

type TestError = Box<dyn std::error::Error + Send + Sync>;

// Bounds every live turn; a stalled gateway is a broken corpus, not a hang.
const LIVE_BOUND: std::time::Duration = std::time::Duration::from_secs(120);

// Streams one turn, folding events as they arrive; returns the folded
// turn plus the raw tool-delta fragment count seen on the wire.
async fn live_assembly(
    provider: &OpenAiProvider,
    request: ModelRequest,
) -> Result<(FoldedTurn, usize), TestError> {
    let tool_deltas = Cell::new(0usize);
    let turn = live_turn_observed(provider, request, |event| {
        if matches!(event, ModelEvent::ToolCallDelta { .. }) {
            tool_deltas.set(tool_deltas.get() + 1);
        }
    })
    .await?;
    Ok((turn, tool_deltas.get()))
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn integration_stream_tools() -> Result<(), TestError> {
    let keys = crate::require_keys!("streamed tool-delta assembly");
    let provider = OpenAiProvider::new(&keys.base_url, &keys.model, &keys.api_key)?;
    let request = ModelRequest {
        system: Some(String::from(
            "You are a weather agent. Call get_weather for weather questions.",
        )),
        messages: vec![std::sync::Arc::new(ChatMessage::User {
            text: String::from("What is the weather in Cairo? Use the tool."),
            images: Vec::new(),
        })],
        tools: vec![ToolSpec {
            name: String::from("get_weather"),
            description: String::from("Current weather for a city."),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
            }),
        }],
        max_tokens: 1024,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    };
    let (turn, tool_deltas) = tokio::time::timeout(LIVE_BOUND, live_assembly(&provider, request))
        .await
        .map_err(|_| String::from("the streamed tool turn exceeded the 120s bound"))??;

    assert!(tool_deltas >= 1, "the stream must carry tool-call deltas");
    assert!(
        !turn.tool_calls.is_empty(),
        "the deltas must fold into complete calls"
    );
    for call in &turn.tool_calls {
        assert!(!call.id.is_empty(), "each folded call must keep its id");
        assert!(!call.name.is_empty(), "each folded call must keep its name");
        assert!(
            serde_json::from_str::<serde_json::Value>(&call.arguments)
                .map(|args| args.get("city").is_some())
                .unwrap_or(false),
            "each folded call's arguments must assemble into JSON carrying the required city"
        );
    }
    println!(
        "assembled {} call(s) from {} tool-delta event(s)",
        turn.tool_calls.len(),
        tool_deltas
    );
    Ok(())
}
