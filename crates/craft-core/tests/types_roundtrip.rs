//! Serde round-trips for the neutral vocabulary: every public data type
//! survives a serialize/deserialize cycle unchanged, and hostile tags
//! deserialize to typed errors instead of panicking.

use craft_core::events::{FoldedTurn, ModelEvent, TurnCaps};
use craft_core::types::{
    ChatMessage, ModelRequest, NativeBlocks, NativeDialect, StopReason, ToolCall, ToolSpec, Usage,
};
use serde::Serialize;
use std::fmt::Debug;

fn round_trip<T>(value: T) -> Result<T, serde_json::Error>
where
    T: Serialize + serde::de::DeserializeOwned + PartialEq + Debug,
{
    let json = serde_json::to_string(&value)?;
    serde_json::from_str(&json)
}

fn sample_tool_call() -> ToolCall {
    ToolCall {
        id: String::from("call_7"),
        name: String::from("bash"),
        arguments: String::from("{\"command\":\"ls\"}"),
    }
}

#[test]
fn model_request_round_trips_with_every_message_role() -> Result<(), serde_json::Error> {
    let request = ModelRequest {
        system: Some(String::from("be terse")),
        messages: vec![
            std::sync::Arc::new(ChatMessage::User {
                text: String::from("list files"),
                images: Vec::new(),
            }),
            std::sync::Arc::new(ChatMessage::Assistant {
                text: String::from("running"),
                tool_calls: vec![sample_tool_call()],
                native_blocks: None,
            }),
            std::sync::Arc::new(ChatMessage::Tool {
                call_id: String::from("call_7"),
                content: String::from("file_a\nfile_b"),
                is_error: false,
                images: Vec::new(),
            }),
        ],
        tools: vec![ToolSpec {
            name: String::from("bash"),
            description: String::from("run a shell command"),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"command": {"type": "string"}}
            }),
        }],
        max_tokens: 4096,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    };
    let json = serde_json::to_string(&request)?;
    assert!(json.contains("\"role\":\"user\""));
    assert!(json.contains("\"role\":\"assistant\""));
    assert!(json.contains("\"role\":\"tool\""));
    assert_eq!(round_trip(request.clone())?, request);
    Ok(())
}

#[test]
fn tool_result_error_flag_round_trips() -> Result<(), serde_json::Error> {
    let message = ChatMessage::Tool {
        call_id: String::from("call_9"),
        content: String::from("command failed"),
        is_error: true,
        images: Vec::new(),
    };
    let json = serde_json::to_string(&message)?;
    assert!(json.contains("\"is_error\":true"));
    assert_eq!(round_trip(message.clone())?, message);
    Ok(())
}

#[test]
fn minimal_model_request_round_trips() -> Result<(), serde_json::Error> {
    let request = ModelRequest {
        system: None,
        messages: Vec::new(),
        tools: Vec::new(),
        max_tokens: 1,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    };
    assert_eq!(round_trip(request.clone())?, request);
    Ok(())
}

#[test]
fn tool_call_round_trips() -> Result<(), serde_json::Error> {
    assert_eq!(round_trip(sample_tool_call())?, sample_tool_call());
    Ok(())
}

#[test]
fn tool_spec_round_trips() -> Result<(), serde_json::Error> {
    let spec = ToolSpec {
        name: String::from("grep"),
        description: String::from("search file contents"),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    assert_eq!(round_trip(spec.clone())?, spec);
    Ok(())
}

#[test]
fn model_events_round_trip() -> Result<(), serde_json::Error> {
    let events = vec![
        ModelEvent::TextDelta {
            text: String::from("par"),
        },
        ModelEvent::ToolCallDelta {
            index: 2,
            id: Some(String::from("call_1")),
            name: None,
            arguments: String::from("{\"a\""),
        },
        ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: Usage::omitted(),
        },
    ];
    assert_eq!(round_trip(events.clone())?, events);
    Ok(())
}

#[test]
fn stop_reasons_round_trip() -> Result<(), serde_json::Error> {
    let reasons = vec![
        StopReason::EndTurn,
        StopReason::MaxTokens,
        StopReason::StopSequence,
        StopReason::ToolUse,
        StopReason::ContentFilter,
        StopReason::Refusal,
        StopReason::PausedTurn,
        StopReason::MalformedToolCall,
        StopReason::ContextWindowExceeded,
        StopReason::Other(String::from("mystery")),
    ];
    assert_eq!(round_trip(reasons.clone())?, reasons);
    Ok(())
}

#[test]
fn folded_turn_and_caps_round_trip() -> Result<(), serde_json::Error> {
    let turn = FoldedTurn {
        text: String::from("done"),
        tool_calls: vec![sample_tool_call()],
        native_blocks: None,
        stop_reason: StopReason::EndTurn,
        usage: Usage::omitted(),
    };
    assert_eq!(round_trip(turn.clone())?, turn);
    let caps = TurnCaps::default();
    assert_eq!(round_trip(caps)?, caps);
    Ok(())
}

#[test]
fn unknown_role_tag_is_a_typed_error() {
    let bad = serde_json::from_str::<ChatMessage>("{\"role\":\"system\",\"text\":\"x\"}");
    assert!(bad.is_err(), "unknown role must not deserialize");
}

#[test]
fn unknown_event_tag_is_a_typed_error() {
    let bad = serde_json::from_str::<ModelEvent>("{\"glitch\":{}}");
    assert!(bad.is_err(), "unknown event tag must not deserialize");
}

#[test]
fn hostile_overflowing_max_tokens_is_rejected_by_serde() {
    let bad = serde_json::from_str::<ModelRequest>(
        "{\"system\":null,\"messages\":[],\"tools\":[],\"max_tokens\":99999999999}",
    );
    assert!(bad.is_err(), "max_tokens must stay within u32");
}

#[test]
fn assistant_native_blocks_round_trip_verbatim() -> Result<(), serde_json::Error> {
    let blocks = serde_json::json!({
        "type": "thinking",
        "thinking": "consider the merge window",
        "signature": "c2lnbmF0dXJl"
    });
    let message = ChatMessage::Assistant {
        text: String::from("merged"),
        tool_calls: Vec::new(),
        native_blocks: Some(NativeBlocks {
            dialect: NativeDialect::Anthropic,
            blocks,
        }),
    };
    let json = serde_json::to_string(&message)?;
    assert!(
        json.contains("\"native_blocks\""),
        "the passthrough must serialize: {json}"
    );
    assert_eq!(round_trip(message.clone())?, message);
    Ok(())
}

#[test]
fn gemini_thought_signature_round_trips() -> Result<(), serde_json::Error> {
    let blocks = serde_json::json!({
        "parts": [{"text": "plan", "thoughtSignature": "c2ln"}]
    });
    let message = ChatMessage::Assistant {
        text: String::new(),
        tool_calls: Vec::new(),
        native_blocks: Some(NativeBlocks {
            dialect: NativeDialect::Gemini,
            blocks,
        }),
    };
    assert_eq!(round_trip(message.clone())?, message);
    Ok(())
}

#[test]
fn assistant_without_native_blocks_omits_the_field() -> Result<(), serde_json::Error> {
    let message = ChatMessage::Assistant {
        text: String::from("plain"),
        tool_calls: Vec::new(),
        native_blocks: None,
    };
    let json = serde_json::to_string(&message)?;
    assert!(
        !json.contains("native_blocks"),
        "None must not serialize: {json}"
    );
    assert_eq!(round_trip(message.clone())?, message);
    Ok(())
}
