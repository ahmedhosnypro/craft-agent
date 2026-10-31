//! Live tool round-trip corpus for the provider seam, driven by the
//! repo-root `.env` (`TEST_BYOK_*`); runs via `just integration_tools`.

#[path = "byok_env.rs"]
mod byok_env;

use craft_core::events::FoldedTurn;
use craft_core::types::{ChatMessage, ModelRequest, ToolCall, ToolSpec};
use craft_providers::openai::OpenAiProvider;

use crate::byok_env::live_turn;

type TestError = Box<dyn std::error::Error + Send + Sync>;

// Bounds every live turn; a stalled gateway is a broken corpus, not a hang.
const LIVE_BOUND: std::time::Duration = std::time::Duration::from_secs(120);

fn weather_tool() -> ToolSpec {
    ToolSpec {
        name: String::from("get_weather"),
        description: String::from("Current weather for a city."),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
        }),
    }
}

fn weather_request(
    system: String,
    messages: Vec<std::sync::Arc<ChatMessage>>,
    tools: Vec<ToolSpec>,
) -> ModelRequest {
    ModelRequest {
        system: Some(system),
        messages,
        tools,
        max_tokens: 1024,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    }
}

fn user_turn(ask: String) -> ChatMessage {
    ChatMessage::User {
        text: ask,
        images: Vec::new(),
    }
}

/// The tool conversation replay: the user ask, the assistant's tool call,
/// and its result.
fn replay_messages(
    ask: String,
    turn: &FoldedTurn,
    call_id: String,
) -> Vec<std::sync::Arc<ChatMessage>> {
    vec![
        std::sync::Arc::new(user_turn(ask)),
        std::sync::Arc::new(ChatMessage::Assistant {
            text: turn.text.clone(),
            tool_calls: turn.tool_calls.clone(),
            native_blocks: None,
        }),
        std::sync::Arc::new(ChatMessage::Tool {
            call_id,
            content: String::from(
                "{\"city\": \"Cairo\", \"temp_c\": 21, \"condition\": \"clear\"}",
            ),
            is_error: false,
            images: Vec::new(),
        }),
    ]
}

async fn bounded_turn(
    provider: &OpenAiProvider,
    request: ModelRequest,
) -> Result<FoldedTurn, TestError> {
    tokio::time::timeout(LIVE_BOUND, live_turn(provider, request))
        .await
        .map_err(|_| String::from("the live turn exceeded the 120s bound"))?
        .map_err(|err| err as TestError)
}

/// The single provider-issued get_weather call the first turn must carry.
fn validated_call(turn: &FoldedTurn) -> Result<ToolCall, TestError> {
    let call = turn
        .tool_calls
        .first()
        .ok_or_else(|| String::from("the declared tool must be called"))?
        .clone();
    assert_eq!(call.name, "get_weather", "the declared tool must be called");
    assert!(
        !call.id.is_empty(),
        "the tool call must carry the provider id"
    );
    assert_eq!(
        turn.tool_calls.len(),
        1,
        "the replay attaches one tool result; extra calls would replay unanswered"
    );
    Ok(call)
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn integration_tools() -> Result<(), TestError> {
    let keys = crate::require_keys!("tool round-trip");
    let provider = OpenAiProvider::new(&keys.base_url, &keys.model, &keys.api_key)?;
    let system = String::from(
        "You are a weather agent. Call get_weather for weather questions; after its result, answer in one short sentence.",
    );
    let ask = String::from("What is the weather in Cairo? Use the tool.");
    let tools = vec![weather_tool()];

    let request = weather_request(
        system.clone(),
        vec![std::sync::Arc::new(user_turn(ask.clone()))],
        tools.clone(),
    );
    let first = bounded_turn(&provider, request).await?;
    let call = validated_call(&first)?;

    let messages = replay_messages(ask, &first, call.id);
    let request = weather_request(system, messages, tools);
    let second = bounded_turn(&provider, request).await?;

    assert!(
        !second.text.trim().is_empty(),
        "the model must finish with an answer after the tool result"
    );
    assert!(
        second.tool_calls.is_empty(),
        "the model must finish instead of re-calling the tool"
    );
    Ok(())
}
