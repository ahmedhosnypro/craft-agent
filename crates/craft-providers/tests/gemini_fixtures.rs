//! Keyless fixture corpus for the Gemini generateContent adapter: request
//! encoding, SSE decode, error taxonomy, and hostile inputs — all served
//! off a loopback mock. Gemini has no live lane in v1; these fixtures are
//! the only contract proof.

#[path = "fixtures/gemini/error_tests.rs"]
mod error_tests;
#[path = "fixtures/gemini/hostile_tests.rs"]
mod hostile_tests;
#[path = "fixtures/gemini/mock.rs"]
mod mock;
#[path = "fixtures/gemini/request_tests.rs"]
mod request_tests;
#[path = "fixtures/gemini/stream_tests.rs"]
mod stream_tests;
#[path = "fixtures/gemini/wire.rs"]
mod wire;

use std::error::Error;

use craft_core::error::ProviderError;
use craft_core::events::{FoldError, FoldedTurn, ModelEvent, TurnCaps, TurnFold};
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest, ToolCall, ToolSpec};
use craft_providers::gemini::GeminiProvider;
use futures::StreamExt;

pub type TestError = Box<dyn Error + Send + Sync>;
pub type Outcome = Result<ModelEvent, ProviderError>;

/// A fixture key; never a live credential.
pub const KEY: &str = "test-key-never-live";
pub const MODEL: &str = "gemini-2.5-flash";

pub fn provider(base_url: &str) -> Result<GeminiProvider, TestError> {
    GeminiProvider::new(base_url, MODEL, KEY).map_err(|err| Box::new(err) as TestError)
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

pub fn tool_call(id: &str, name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: String::from(id),
        name: String::from(name),
        arguments: String::from(arguments),
    }
}

pub fn sample_request() -> ModelRequest {
    ModelRequest {
        system: Some(String::from("You are a digest agent.")),
        messages: vec![ChatMessage::User {
            text: String::from("Summarize the merged PRs."),
        }],
        tools: Vec::new(),
        max_tokens: 128,
    }
}

/// A conversation replay: user text, an assistant tool call, its result.
pub fn replay_request(assistant_text: &str) -> ModelRequest {
    ModelRequest {
        system: None,
        messages: vec![
            ChatMessage::User {
                text: String::from("List the failing checks."),
            },
            ChatMessage::Assistant {
                text: String::from(assistant_text),
                tool_calls: vec![ToolCall {
                    id: String::from("call_a"),
                    name: String::from("bash"),
                    arguments: String::from("{\"cmd\":\"ls\"}"),
                }],
                native_blocks: None,
            },
            ChatMessage::Tool {
                call_id: String::from("call_a"),
                content: String::from("0 files"),
                is_error: true,
            },
        ],
        tools: vec![tool_spec("bash")],
        max_tokens: 128,
    }
}

/// Drives one adapter stream to completion; a failed connect surfaces as
/// the single error outcome.
pub async fn run(provider: &GeminiProvider, request: ModelRequest) -> Vec<Outcome> {
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
