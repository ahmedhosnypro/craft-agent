//! Stationarity corpus: identical repeated tool calls (canonicalized
//! signature) nudge after 4 and stop after 8.

mod common;

use common::*;
use craft_core::types::{ChatMessage, StopReason};
use craft_runtime::{CancelCategory, RunConfig, Runner};

fn config() -> RunConfig {
    RunConfig::new(std::path::PathBuf::from("."))
}

fn nudge_count(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(m, ChatMessage::User { text } if text.contains("repeated")))
        .count()
}

#[tokio::test]
async fn fourth_identical_call_injects_a_nudge_user_message() {
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "out"),
        config(),
    );
    let (outcome, _) = run_collect(&runner, "loop").await;

    assert_eq!(outcome.exit_code(), 0);
    let requests = provider.requests();
    // The request after the 4th identical call carries exactly one nudge.
    assert_eq!(nudge_count(&requests[4].messages), 1);
    assert!(matches!(
        requests[4].messages.last(),
        Some(ChatMessage::User { text }) if text.contains("repeated")
    ));
    // Earlier requests carry none.
    for request in &requests[..4] {
        assert_eq!(nudge_count(&request.messages), 0);
    }
}

#[tokio::test]
async fn eighth_identical_call_stops_the_run_with_stationarity_category() {
    let provider = FakeProvider::new(vec![tool_turn("c1", "scripted", "{}", usage(1, 1))]);
    let runner = Runner::new(provider, registry_with_ok("scripted", "out"), config());
    let (outcome, events) = run_collect(&runner, "loop forever").await;

    assert_eq!(outcome.exit_code(), 2);
    assert_eq!(outcome.category, CancelCategory::Stationarity);
    assert_eq!(outcome.turns, 8);
    assert!(outcome.detail.contains("stationary"));
    assert!(matches!(
        events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished { exit_code, category, .. })
        if *exit_code == 2 && *category == CancelCategory::Stationarity
    ));
}

#[tokio::test]
async fn changing_arguments_resets_the_streak() {
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        tool_turn("c2", "scripted", "{ }", usage(1, 1)),
        tool_turn("c3", "scripted", r#"{"a":1}"#, usage(1, 1)),
        tool_turn("c4", "scripted", r#"{"a": 1}"#, usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "out"),
        config(),
    );
    let (outcome, _) = run_collect(&runner, "vary").await;

    assert_eq!(outcome.exit_code(), 0);
    for request in provider.requests() {
        assert_eq!(nudge_count(&request.messages), 0);
    }
}
