//! Runner corpus for provider-native blocks: a turn the fold carried
//! native blocks for must land them in the session record and carry
//! them into every later request verbatim, while native-free traffic
//! keeps a byte-stable record.

mod common;

use common::*;
use craft_core::events::ModelEvent;
use craft_core::types::{ChatMessage, NativeBlocks, NativeDialect, StopReason, Usage};
use craft_runtime::{RunConfig, Runner};
use std::path::PathBuf;

fn blocks() -> NativeBlocks {
    NativeBlocks {
        dialect: NativeDialect::Anthropic,
        blocks: serde_json::json!([
            {"type": "thinking", "thinking": "hidden reasoning", "signature": "sig-1"}
        ]),
    }
}

fn config(session: PathBuf) -> RunConfig {
    let mut cfg = RunConfig::new(PathBuf::from("."));
    cfg.session_path = Some(session);
    cfg
}

fn lines(path: &PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// A native block followed by one tool call: the loop continues so the
/// next request is built with the blocks on board.
fn native_tool_turn(id: &str, used: Usage) -> Turn {
    let block = serde_json::json!({
        "type": "thinking", "thinking": "hidden reasoning", "signature": "sig-1"
    });
    Turn::Events(vec![
        Ok(ModelEvent::NativeBlock {
            dialect: NativeDialect::Anthropic,
            block,
        }),
        Ok(ModelEvent::ToolCallDelta {
            index: 0,
            id: Some(String::from(id)),
            name: Some(String::from("scripted")),
            arguments: String::from("{}"),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: used,
        }),
    ])
}

/// A native block and nothing else: the run ends after this turn.
fn native_end_turn(used: Usage) -> Turn {
    let block = serde_json::json!({
        "type": "thinking", "thinking": "hidden reasoning", "signature": "sig-1"
    });
    Turn::Events(vec![
        Ok(ModelEvent::NativeBlock {
            dialect: NativeDialect::Anthropic,
            block,
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::EndTurn,
            usage: used,
        }),
    ])
}

fn assistant_with_blocks() -> ChatMessage {
    ChatMessage::Assistant {
        text: String::new(),
        tool_calls: Vec::new(),
        native_blocks: Some(blocks()),
    }
}

#[tokio::test]
async fn folded_native_blocks_land_in_the_session_record() {
    let path = session_path("native-fold");
    let runner = Runner::new(
        FakeProvider::new(vec![native_end_turn(usage(1, 1))]),
        registry_with_ok("scripted", "out"),
        config(path.clone()),
    );
    let (outcome, _) = run_collect(&runner, "think").await;
    assert_eq!(outcome.exit_code(), 0);

    let logged: Vec<ChatMessage> = lines(&path)
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(logged.len(), 2);
    assert!(matches!(&logged[0], ChatMessage::User { text, .. } if text == "think"));
    assert_eq!(logged[1], assistant_with_blocks());
}

#[tokio::test]
async fn the_next_request_carries_the_native_blocks_through_trimming() {
    let provider = FakeProvider::new(vec![
        native_tool_turn("c1", usage(1, 1)),
        native_tool_turn("c2", usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let mut cfg = RunConfig::new(PathBuf::from("."));
    cfg.result_window_turns = Some(1);
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", "out"), cfg);
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let third = &requests[2];
    assert_eq!(third.messages.len(), 5);
    assert!(matches!(
        &third.messages[1],
        ChatMessage::Assistant { native_blocks, .. } if *native_blocks == Some(blocks())
    ));
    assert!(matches!(
        &third.messages[2],
        ChatMessage::Tool { content, .. } if content.contains("trimmed")
    ));
    assert!(matches!(
        &third.messages[3],
        ChatMessage::Assistant { native_blocks, .. } if *native_blocks == Some(blocks())
    ));
}

#[tokio::test]
async fn native_free_turns_keep_the_record_byte_stable() {
    let path = session_path("native-free");
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "out"),
        config(path.clone()),
    );
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let logged: Vec<ChatMessage> = lines(&path)
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(logged.len(), 4);
    assert_eq!(
        logged[3],
        ChatMessage::Assistant {
            text: String::from("done"),
            tool_calls: Vec::new(),
            native_blocks: None,
        }
    );
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    for line in raw.lines().filter(|line| line.contains("\"assistant\"")) {
        assert!(!line.contains("native_blocks"), "got {line}");
    }
    assert!(matches!(
        &provider.requests()[1].messages[1],
        ChatMessage::Assistant {
            native_blocks: None,
            ..
        }
    ));
}
