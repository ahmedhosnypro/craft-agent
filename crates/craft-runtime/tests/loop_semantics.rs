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
//  11  salvaged marker arguments: the call never executes; the model
//      sees an is_error result naming the raw fragment
//
// Scenarios 5-6 live in budget.rs, 7 in stationarity.rs, 8 in
// denial.rs, 9 in session_log.rs, 10 in stream_json.rs. The step ladder
// lives in step_retry.rs; this corpus pins fail-fast (one attempt).

mod common;

use common::*;
use craft_core::types::{ChatMessage, StopReason};
use craft_runtime::{CancelCategory, RunConfig, RunOutcome, Runner};

fn config() -> RunConfig {
    let mut config = RunConfig::new(std::path::PathBuf::from("."));
    config.step_attempts = Some(1);
    config.step_patience = Some(std::time::Duration::ZERO);
    config
}

/// One transcript message that is an assistant turn requesting exactly
/// one tool call.
fn is_one_tool_call(message: &std::sync::Arc<ChatMessage>) -> bool {
    matches!(
        message.as_ref(),
        ChatMessage::Assistant { tool_calls, .. } if tool_calls.len() == 1
    )
}

/// The transcript messages of the request at `index`.
fn request_messages(provider: &FakeProvider, index: usize) -> Vec<std::sync::Arc<ChatMessage>> {
    provider.requests()[index].messages.clone()
}

/// One transcript message that is a tool result for `call_id`, carrying
/// `output` and no error flag.
fn is_tool_result_of(message: &std::sync::Arc<ChatMessage>, call_id: &str, output: &str) -> bool {
    matches!(message.as_ref(), ChatMessage::Tool {
        call_id: id, is_error, content, ..
    } if id == call_id && !*is_error && content == output)
}

/// One transcript message that is an error tool result.
fn is_tool_error(message: &std::sync::Arc<ChatMessage>) -> bool {
    matches!(message.as_ref(), ChatMessage::Tool { is_error, .. } if *is_error)
}

/// One transcript message that is an error tool result naming `needle`.
fn is_tool_error_naming(message: &std::sync::Arc<ChatMessage>, needle: &str) -> bool {
    matches!(message.as_ref(), ChatMessage::Tool { is_error, content, .. }
        if *is_error && content.contains(needle))
}

/// A non-error tool result carrying `output` anywhere in the stream.
fn saw_tool_result(events: &[craft_runtime::RuntimeEvent], output: &str) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            craft_runtime::RuntimeEvent::ToolResult { is_error, content, .. }
            if !*is_error && content == output
        )
    })
}

/// An error tool result anywhere in the stream.
fn saw_tool_error(events: &[craft_runtime::RuntimeEvent]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            craft_runtime::RuntimeEvent::ToolResult { is_error, .. } if *is_error
        )
    })
}

/// Every tool result the stream carried: (is_error, content).
fn tool_results(events: &[craft_runtime::RuntimeEvent]) -> Vec<(bool, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            craft_runtime::RuntimeEvent::ToolResult {
                is_error, content, ..
            } => Some((*is_error, content.clone())),
            _ => None,
        })
        .collect()
}

/// Whether the run finished with this exit code and category.
fn is_finished_with(
    events: &[craft_runtime::RuntimeEvent],
    exit_code: i32,
    category: CancelCategory,
) -> bool {
    matches!(events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished { exit_code: code, category: seen, .. })
        if *code == exit_code && *seen == category)
}

/// Asserts the run completed after `turns` turns.
fn assert_completed(outcome: &RunOutcome, turns: u32) {
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.category, CancelCategory::Completed);
    assert_eq!(outcome.turns, turns);
}

/// Asserts the run's charged usage is exactly this and complete.
fn assert_usage(outcome: &RunOutcome, input: u64, output: u64) {
    assert_eq!(outcome.usage.input_tokens, input);
    assert_eq!(outcome.usage.output_tokens, output);
    assert!(!outcome.usage.incomplete);
}

/// Asserts the request at `index` replays the tool call with its result.
fn assert_result_replayed(provider: &FakeProvider, index: usize, call_id: &str, output: &str) {
    let messages = request_messages(provider, index);
    assert_eq!(messages.len(), 3);
    assert!(is_one_tool_call(&messages[1]));
    assert!(is_tool_result_of(&messages[2], call_id, output));
}

/// Asserts the run aborted as a plain error naming `needle`.
fn assert_error_abort(outcome: &RunOutcome, needle: &str) {
    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(outcome.category, CancelCategory::Error);
    assert!(outcome.detail.contains(needle));
}

/// Asserts the only tool result is the salvage feedback naming `fragment`.
fn assert_salvage_feedback(results: &[(bool, String)], fragment: &str) {
    assert_eq!(results.len(), 1, "one tool result");
    assert!(results[0].0, "the marker call is an is_error result");
    let expected =
        format!("tool arguments were not valid JSON; the call was not executed: {fragment}");
    assert_eq!(results[0].1, expected);
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

    assert_completed(&outcome, 2);
    assert_usage(&outcome, 14, 7);

    // The tool result went back to the model on the next request.
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_result_replayed(&provider, 1, "call-1", "tool output");

    // Stream-json saw the tool call, the result, and a clean finish.
    assert!(saw_tool_result(&events, "tool output"));
    assert!(is_finished_with(&events, 0, CancelCategory::Completed));
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
    assert!(is_tool_error_naming(
        &requests[1].messages[2],
        "file not found"
    ));
    assert!(saw_tool_error(&events));
}

#[tokio::test]
async fn empty_response_aborts_with_exit_one() {
    let provider = FakeProvider::new(vec![usage_only_turn(usage(3, 1))]);
    let runner = Runner::new(provider, registry_with_ok("scripted", "x"), config());
    let (outcome, events) = run_collect(&runner, "say something").await;

    assert_error_abort(&outcome, "empty response");
    assert!(is_finished_with(&events, 1, CancelCategory::Error));
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

/// A call whose arguments are the fold's salvage marker never reaches
/// the tool: the model sees an is_error result naming the raw fragment
/// and the run continues.
#[tokio::test]
async fn salvaged_arguments_short_circuit_the_tool_with_feedback() {
    let marker = craft_core::types::salvage_arguments("not json {");
    let provider = FakeProvider::new(vec![
        tool_turn("call-1", "scripted", &marker, usage(2, 1)),
        text_turn("recovered", StopReason::EndTurn, usage(2, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "tool output"),
        config(),
    );
    let (outcome, events) = run_collect(&runner, "use the tool").await;

    assert_eq!(outcome.exit_code(), 0);
    let results = tool_results(&events);
    assert_salvage_feedback(&results, "not json {");

    let replayed = request_messages(&provider, 1);
    assert!(
        is_tool_error(&replayed[2]),
        "the feedback rides back to the model"
    );
}

/// Every request assembles the transcript by sharing the session's
/// entries: the same `Arc` rides each turn's request, so per-request
/// assembly bumps pointers instead of deep-cloning messages.
#[tokio::test]
async fn each_request_shares_the_transcripts_entries() {
    let provider = FakeProvider::new(vec![
        tool_turn("call-1", "scripted", "{}", usage(2, 1)),
        text_turn("done", StopReason::EndTurn, usage(2, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "out"),
        config(),
    );
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let first = &requests[0].messages;
    let second = &requests[1].messages;
    assert_eq!(second.len(), first.len() + 2);
    for (index, (before, later)) in first.iter().zip(second.iter()).enumerate() {
        assert!(
            std::sync::Arc::ptr_eq(before, later),
            "message {index} is the transcript's shared entry, not a fresh copy"
        );
    }
}
