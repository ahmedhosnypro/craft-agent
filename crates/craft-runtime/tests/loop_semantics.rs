//! Loop-semantics corpus: deterministic, keyless, offline.
//!
//! A scripted fake provider drives the real tool registry through the
//! Runner; each test pins one clause of the headless agent-loop contract.

// Corpus index (one scenario per test):
//   1  multi-turn tool round-trip: tool_call -> tool_result -> finish
//   2  tool error -> tool_result is_error:true round-trip
//   3  EmptyResponse -> exit-1 family
//   4  provider stream error -> typed propagation, never a panic
//      (+ idle-timeout bound on a stalled stream)
//   5  max-turns abort: exit 2 + cancellation category
//   6  max-total-tokens abort: exit 3; fail-closed on incomplete usage
//   7  stationarity: nudge after 4 identical calls, stop after 8
//   8  sensitive-path denial -> exit 4
//   9  session JSONL: model-visible <=> logged; native-block replay
//  10  stream-json: every NDJSON line schema-valid; terminal event
//      carries exit code + category + total usage
//
// Scenarios 5-6 live in budget.rs, 7 in stationarity.rs, 8 in
// denial.rs, 9 in session_log.rs, 10 in stream_json.rs. The step ladder
// lives in step_retry.rs; this corpus pins fail-fast (one attempt).

mod common;

use common::*;
use craft_core::types::{ChatMessage, StopReason};
use craft_runtime::{CancelCategory, RunConfig, Runner};

fn config() -> RunConfig {
    let mut config = RunConfig::new(std::path::PathBuf::from("."));
    config.step_attempts = Some(1);
    config
}

#[tokio::test]
async fn tool_round_trip_feeds_result_back_and_finishes() {
    let provider = FakeProvider::new(vec![
        tool_turn("call-1", "scripted", "{}", usage(10, 5)),
        text_turn("all done", StopReason::EndTurn, usage(4, 2)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "tool output"),
        config(),
    );
    let (outcome, events) = run_collect(&runner, "use the tool").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.category, CancelCategory::Completed);
    assert_eq!(outcome.turns, 2);
    assert_eq!(outcome.usage.input_tokens, 14);
    assert_eq!(outcome.usage.output_tokens, 7);
    assert!(!outcome.usage.incomplete);

    // The tool result went back to the model on the next request.
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let second = &requests[1].messages;
    assert_eq!(second.len(), 3);
    assert!(matches!(&second[1], ChatMessage::Assistant {
        tool_calls, ..
    } if tool_calls.len() == 1));
    assert!(matches!(&second[2], ChatMessage::Tool {
        call_id, is_error, content, ..
    } if call_id == "call-1" && !*is_error && content == "tool output"));

    // Stream-json saw the tool call, the result, and a clean finish.
    assert!(events.iter().any(|e| matches!(
        e,
        craft_runtime::RuntimeEvent::ToolResult { is_error, content, .. }
        if !*is_error && content == "tool output"
    )));
    assert!(matches!(
        events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished { exit_code, .. })
        if *exit_code == 0
    ));
}

#[tokio::test]
async fn tool_error_round_trips_as_is_error_tool_result() {
    let provider = FakeProvider::new(vec![
        tool_turn("call-9", "scripted", "{}", usage(2, 1)),
        text_turn("recovered", StopReason::EndTurn, usage(2, 1)),
    ]);
    let registry = registry_with_error(
        "scripted",
        craft_tools::ToolError::NotFound {
            path: String::from("nope.txt"),
        },
    );
    let runner = Runner::new(provider.clone(), registry, config());
    let (outcome, events) = run_collect(&runner, "use the tool").await;

    assert_eq!(outcome.exit_code(), 0);
    let requests = provider.requests();
    assert!(matches!(&requests[1].messages[2], ChatMessage::Tool {
        is_error, content, ..
    } if *is_error && content.contains("file not found")));
    assert!(events.iter().any(|e| matches!(
        e,
        craft_runtime::RuntimeEvent::ToolResult { is_error, .. } if *is_error
    )));
}

#[tokio::test]
async fn empty_response_aborts_with_exit_one() {
    let provider = FakeProvider::new(vec![usage_only_turn(usage(3, 1))]);
    let runner = Runner::new(provider, registry_with_ok("scripted", "x"), config());
    let (outcome, events) = run_collect(&runner, "say something").await;

    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(outcome.category, CancelCategory::Error);
    assert!(outcome.detail.contains("empty response"));
    assert!(matches!(
        events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished { exit_code, category, .. })
        if *exit_code == 1 && *category == CancelCategory::Error
    ));
}

#[tokio::test]
async fn provider_stream_error_propagates_typed_never_panics() {
    for turn in [
        failing_turn("socket closed"),
        stream_error_turn("wire died mid-stream"),
    ] {
        let provider = FakeProvider::new(vec![turn]);
        let runner = Runner::new(provider, registry_with_ok("scripted", "x"), config());
        let (outcome, _) = run_collect(&runner, "go").await;
        assert_eq!(outcome.exit_code(), 1);
        assert_eq!(outcome.category, CancelCategory::Error);
        assert!(
            outcome.detail.contains("socket closed")
                || outcome.detail.contains("wire died mid-stream")
        );
    }
}

#[tokio::test]
async fn idle_timeout_bounds_a_stalled_stream() {
    let provider = FakeProvider::new(vec![Turn::Stall]);
    let mut cfg = config();
    cfg.idle_timeout = Some(std::time::Duration::from_millis(50));
    let runner = Runner::new(provider, registry_with_ok("scripted", "x"), cfg);
    let (outcome, _) = run_collect(&runner, "stall").await;

    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(outcome.category, CancelCategory::Error);
    assert!(outcome.detail.contains("idle"));
}
