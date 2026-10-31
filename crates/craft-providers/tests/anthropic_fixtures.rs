//! Anthropic Messages adapter corpus: request encoding, named-event SSE
//! decode, tool-input assembly, accounting frames, error rows, and
//! hostile inputs — all served off a loopback mock.

#[path = "fixtures/anthropic/error_tests.rs"]
mod error_tests;
#[path = "fixtures/anthropic/hostile_tests.rs"]
mod hostile_tests;
#[path = "fixtures/mock.rs"]
mod mock;
#[path = "fixtures/anthropic/request_feature_tests.rs"]
mod request_feature_tests;
#[path = "fixtures/anthropic/request_media_tests.rs"]
mod request_media_tests;
#[path = "fixtures/anthropic/request_tests.rs"]
mod request_tests;
#[path = "fixtures/anthropic/stream_tests.rs"]
mod stream_tests;
#[path = "fixtures/anthropic/wire.rs"]
mod wire;

use std::error::Error;
use std::time::Duration;

use craft_core::error::ProviderError;
use craft_core::events::{FoldError, FoldedTurn, ModelEvent, TurnCaps, TurnFold};
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest, ToolCall, ToolSpec};
use craft_providers::anthropic::AnthropicProvider;
use futures::StreamExt;

pub type TestError = Box<dyn Error + Send + Sync>;
pub type Outcome = Result<ModelEvent, ProviderError>;

/// A fixture key; never a live credential.
pub const KEY: &str = "test-key-never-live";
pub const MODEL: &str = "claude-haiku-5-5";

pub fn provider(base_url: &str) -> Result<AnthropicProvider, TestError> {
    // The ladder rows pin attempt-bound semantics: the patience
    // window is disabled in the fixtures.
    let provider =
        AnthropicProvider::new(base_url, MODEL, KEY).map_err(|err| Box::new(err) as TestError)?;
    Ok(provider.with_retry_patience(Duration::ZERO))
}

pub fn tool_spec(name: &str) -> ToolSpec {
    ToolSpec {
        name: String::from(name),
        description: format!("{name} tool for fixtures"),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
        }),
    }
}

pub fn sample_request() -> ModelRequest {
    ModelRequest {
        system: Some(String::from("You are a digest agent.")),
        messages: vec![std::sync::Arc::new(ChatMessage::User {
            text: String::from("Summarize the merged PRs."),
            images: Vec::new(),
        })],
        tools: Vec::new(),
        max_tokens: 128,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    }
}

/// A conversation replay: user text, an assistant tool call, its result —
/// the result rides as an error row, per the pairing invariant.
pub fn replay_request() -> ModelRequest {
    ModelRequest {
        system: None,
        messages: vec![
            std::sync::Arc::new(ChatMessage::User {
                text: String::from("List the failing checks."),
                images: Vec::new(),
            }),
            std::sync::Arc::new(ChatMessage::Assistant {
                text: String::from("Checking now."),
                tool_calls: vec![ToolCall {
                    id: String::from("toolu_a"),
                    name: String::from("bash"),
                    arguments: String::from("{\"cmd\":\"ls\"}"),
                }],
                native_blocks: None,
            }),
            std::sync::Arc::new(ChatMessage::Tool {
                call_id: String::from("toolu_a"),
                content: String::from("0 files"),
                is_error: true,
                images: Vec::new(),
            }),
        ],
        tools: vec![tool_spec("bash")],
        max_tokens: 128,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    }
}

/// Drives one adapter stream to completion; a failed connect surfaces as
/// the single error outcome.
pub async fn run(provider: &AnthropicProvider, request: ModelRequest) -> Vec<Outcome> {
    let mut stream = match provider.stream(request).await {
        Ok(stream) => stream,
        Err(err) => return vec![Err(err)],
    };
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item);
    }
    events
}

pub fn fold(events: &[Outcome]) -> Result<Result<FoldedTurn, FoldError>, TestError> {
    fold_with_caps(events, TurnCaps::default())
}

/// Folds a stream of ok events; a stream error is a test failure and a
/// fold error is the row's outcome.
pub fn fold_with_caps(
    events: &[Outcome],
    caps: TurnCaps,
) -> Result<Result<FoldedTurn, FoldError>, TestError> {
    let mut turn = TurnFold::new(caps);
    for event in events {
        let event = event
            .as_ref()
            .map_err(|err| Box::new(err.clone()) as TestError)?;
        if let Err(err) = turn.push(event.clone()) {
            return Ok(Err(err));
        }
    }
    Ok(turn.finish())
}

/// The turn a fixture stream must fold into.
pub fn folded(events: &[Outcome]) -> Result<FoldedTurn, TestError> {
    match fold(events)? {
        Ok(turn) => Ok(turn),
        Err(err) => Err(format!("the stream must fold into a turn: {err}").into()),
    }
}

pub fn terminal_count(events: &[Outcome]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, Ok(ModelEvent::TurnFinished { .. })))
        .count()
}

pub fn errors(events: &[Outcome]) -> Vec<ProviderError> {
    events
        .iter()
        .filter_map(|event| event.as_ref().err().cloned())
        .collect()
}

/// The first typed error of a stream, as the row's surfaced failure.
pub fn surfaced(events: &[Outcome]) -> Result<ProviderError, TestError> {
    errors(events)
        .into_iter()
        .next()
        .ok_or_else(|| String::from("the stream must surface a typed error").into())
}

/// The detail text of the variants that carry one.
pub fn detail_of(err: &ProviderError) -> &str {
    match err {
        ProviderError::Auth { detail }
        | ProviderError::Transient { detail }
        | ProviderError::PayloadTooLarge { detail }
        | ProviderError::ContextWindow { detail }
        | ProviderError::InvalidRequest { detail }
        | ProviderError::Transport { detail } => detail,
        _ => "",
    }
}
