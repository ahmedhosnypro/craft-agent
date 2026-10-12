//! Live streamed tool-delta assembly corpus, driven by the repo-root
//! `.env` (`TEST_BYOK_*`); runs via `just integration_stream_tools`.

#[path = "byok_env.rs"]
mod byok_env;

use std::error::Error;
use std::time::Duration;

use craft_core::events::{FoldedTurn, ModelEvent, TurnCaps, TurnFold};
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest, ToolSpec};
use craft_providers::openai::OpenAiProvider;
use futures::StreamExt;

type TestError = Box<dyn Error + Send + Sync>;

// Bounds every live turn; a stalled gateway is a broken corpus, not a hang.
const LIVE_BOUND: Duration = Duration::from_secs(120);

// Streams one turn, folding events as they arrive; returns the folded
// turn plus the raw tool-delta fragment count seen on the wire.
async fn live_assembly(
    provider: &OpenAiProvider,
    request: ModelRequest,
) -> Result<(FoldedTurn, usize), TestError> {
    let mut stream = provider.stream(request).await?;
    let mut fold = TurnFold::new(TurnCaps::default());
    let mut tool_deltas = 0usize;
    while let Some(item) = stream.next().await {
        let event = item?;
        if matches!(event, ModelEvent::ToolCallDelta { .. }) {
            tool_deltas += 1;
        }
        fold.push(event)?;
    }
    Ok((fold.finish()?, tool_deltas))
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
        messages: vec![ChatMessage::User {
            text: String::from("What is the weather in Cairo? Use the tool."),
        }],
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
