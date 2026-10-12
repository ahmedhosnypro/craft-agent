//! Denial corpus: a tool call on a banned path aborts the run with the
//! exit-4 family and its cancellation category.

mod common;

use common::*;
use craft_core::events::ModelEvent;
use craft_core::types::{ChatMessage, StopReason};
use craft_runtime::{CancelCategory, RunConfig, Runner};
use std::path::PathBuf;

fn config() -> RunConfig {
    RunConfig::new(std::path::PathBuf::from("."))
}

fn config_with_session(path: PathBuf) -> RunConfig {
    let mut cfg = config();
    cfg.session_path = Some(path);
    cfg
}

#[tokio::test]
async fn sensitive_path_denial_aborts_with_exit_four() {
    let provider = FakeProvider::new(vec![tool_turn(
        "c1",
        "read_file",
        r#"{"path":".env"}"#,
        usage(1, 1),
    )]);
    let runner = Runner::new(provider, builtin_registry(), config());
    let (outcome, events) = run_collect(&runner, "read the secrets").await;

    assert_eq!(outcome.exit_code(), 4);
    assert_eq!(outcome.category, CancelCategory::SensitivePath);
    assert!(outcome.detail.contains("sensitive"));
    assert!(matches!(
        events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished { exit_code, category, .. })
        if *exit_code == 4 && *category == CancelCategory::SensitivePath
    ));
}

#[tokio::test]
async fn workspace_escape_denial_aborts_with_exit_four() {
    let provider = FakeProvider::new(vec![tool_turn(
        "c1",
        "read_file",
        r#"{"path":"../outside.txt"}"#,
        usage(1, 1),
    )]);
    let runner = Runner::new(provider, builtin_registry(), config());
    let (outcome, _) = run_collect(&runner, "escape").await;

    assert_eq!(outcome.exit_code(), 4);
    assert_eq!(outcome.category, CancelCategory::EscapesWorkspace);
    assert!(outcome.detail.contains("workspace"));
}

#[tokio::test]
async fn a_denial_answers_the_denied_and_unstarted_calls_in_the_record() {
    let path = session_path("denial-record");
    let mixed_turn = Turn::Events(vec![
        Ok(ModelEvent::ToolCallDelta {
            index: 0,
            id: Some(String::from("c1")),
            name: Some(String::from("read_file")),
            arguments: String::from(r#"{"path":"Cargo.toml"}"#),
        }),
        Ok(ModelEvent::ToolCallDelta {
            index: 1,
            id: Some(String::from("c2")),
            name: Some(String::from("read_file")),
            arguments: String::from(r#"{"path":".env"}"#),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: usage(1, 1),
        }),
    ]);
    let provider = FakeProvider::new(vec![mixed_turn]);
    let runner = Runner::new(
        provider,
        builtin_registry(),
        config_with_session(path.clone()),
    );
    let (outcome, _) = run_collect(&runner, "mixed batch").await;
    assert_eq!(outcome.exit_code(), 4);

    let record: Vec<ChatMessage> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let results = &record[record.len() - 2..];
    assert!(matches!(
        &results[0],
        ChatMessage::Tool {
            call_id,
            is_error: false,
            ..
        } if call_id == "c1"
    ));
    assert!(matches!(
        &results[1],
        ChatMessage::Tool {
            call_id,
            is_error: true,
            content,
            ..
        } if call_id == "c2" && content.contains("sensitive path denial")
    ));
}
