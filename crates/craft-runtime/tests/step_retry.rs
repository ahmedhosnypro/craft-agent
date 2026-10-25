//! Step-ladder corpus: deterministic, keyless, offline.
//!
//! A scripted fake provider drives the real Runner through the runtime
//! step ladder. Paused tokio time keeps every backoff instant. Corpus
//! index (one scenario per test):
//!
//!   1  mid-stream close N times, then success: the step replays from
//!      the intact transcript and the turn_retried events pin their shape
//!   2  an always-dying lane stops after exactly the knob's attempts
//!   3  connect-phase failures never step-retry (the one-level rule)
//!   4  deterministic in-stream classes never step-retry
//!   5  fold failures are semantic and never step-retry
//!   6  empty-but-complete turns replay until content; their usage
//!      stays charged to the run
//!   7  a filtered empty turn fails without one replay
//!   8  the budget guard wins over a replay: exit 3, never a loop
//!   9  a stream that ends at EOF without the terminal event replays
//!      on the same ladder
//!
//! The CRAFT_AGENT_STEP_RETRIES knob rows live in step_retry_knob.rs.

mod common;

use common::*;
use craft_core::error::ProviderError;
use craft_core::types::StopReason;
use craft_runtime::RuntimeEvent;
use craft_runtime::{CancelCategory, RunConfig, RunOutcome, Runner};
use std::path::PathBuf;
use std::time::Duration;

fn config(attempts: u32) -> RunConfig {
    let mut config = RunConfig::new(PathBuf::from("."));
    config.step_attempts = Some(attempts);
    // The attempt-bound rows: the patience window is disabled so the
    // knob alone bounds the ladder.
    config.step_patience = Some(Duration::ZERO);
    config
}

fn retried(events: &[RuntimeEvent]) -> Vec<(u32, u32, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::TurnRetried {
                turn,
                attempt,
                reason,
            } => Some((*turn, *attempt, reason.clone())),
            _ => None,
        })
        .collect()
}

fn wire(requests: &[craft_core::types::ModelRequest], index: usize) -> String {
    let messages: Vec<&craft_core::types::ChatMessage> =
        requests[index].messages.iter().map(AsRef::as_ref).collect();
    serde_json::to_string(&messages).unwrap_or_default()
}

/// Asserts the typed error conclusion without touching the ladder.
fn assert_error_exit(outcome: &RunOutcome, fragment: &str) {
    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(outcome.category, CancelCategory::Error);
    assert!(outcome.detail.contains(fragment), "{outcome:?}");
}

/// Asserts the budget conclusion with its exact spend line.
fn assert_budget_exit(outcome: &RunOutcome, spent: u64, allowed: u64) {
    assert_eq!(outcome.exit_code(), 3);
    assert_eq!(outcome.category, CancelCategory::Budget);
    let line = format!("token budget exceeded: {spent} tokens spent, {allowed} allowed");
    assert!(outcome.detail.contains(&line), "{outcome:?}");
}

/// Asserts the run's total charged usage.
fn assert_usage(outcome: &RunOutcome, input: u64, output: u64) {
    assert_eq!(outcome.usage.input_tokens, input);
    assert_eq!(outcome.usage.output_tokens, output);
}

/// Asserts one ladder retry on turn one.
fn assert_single_retry(events: &[RuntimeEvent], reason: &str) {
    assert_eq!(retried(events), vec![(1, 1, String::from(reason))]);
}

/// Asserts two ladder retries on turn one, both with the same reason.
fn assert_two_retries(events: &[RuntimeEvent], reason: &str) {
    assert_eq!(
        retried(events),
        vec![(1, 1, String::from(reason)), (1, 2, String::from(reason)),]
    );
}

/// Every replay sent the same request: the dead attempts appended
/// nothing to the transcript.
fn assert_intact_transcript(provider: &FakeProvider) {
    let requests = provider.requests();
    assert_eq!(requests.len(), 3);
    let transcript = wire(&requests, 0);
    assert_eq!(wire(&requests, 1), transcript);
    assert_eq!(wire(&requests, 2), transcript);
}

/// The ladder's event shape: each dead attempt streamed its partial
/// delta and was voided by turn_retried; the final attempt's text
/// stands.
fn assert_ladder_events(events: &[RuntimeEvent]) {
    assert_eq!(
        serde_json::to_string(&events[2]).unwrap_or_default(),
        r#"{"type":"turn_retried","turn":1,"attempt":1,"reason":"stream_closed"}"#,
        "the retry event shape is pinned"
    );
    let delta_count = events
        .iter()
        .filter(
            |event| matches!(event, RuntimeEvent::AssistantDelta { text, .. } if text == "partial"),
        )
        .count();
    assert_eq!(delta_count, 2, "both dead attempts streamed their partial");
    assert!(events.iter().any(|event| matches!(
        event,
        RuntimeEvent::AssistantDelta { text, .. } if text == "done"
    )));
}

#[tokio::test(start_paused = true)]
async fn midstream_close_replays_the_step_from_the_intact_transcript() {
    let dead = ProviderError::StreamClosed;
    let provider = FakeProvider::new(vec![
        dying_turn(dead.clone()),
        dying_turn(dead),
        text_turn("done", StopReason::EndTurn, usage(2, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(3),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.turns, 1);
    assert_eq!(outcome.usage.input_tokens, 2);

    assert_intact_transcript(&provider);
    assert_two_retries(&events, "stream_closed");
    assert_ladder_events(&events);
    assert_eq!(
        events.last(),
        Some(&RuntimeEvent::RunFinished {
            exit_code: 0,
            category: CancelCategory::Completed,
            detail: String::from("run completed"),
            usage: usage(2, 1),
            turns: 1,
        })
    );
}

#[tokio::test(start_paused = true)]
async fn an_always_dying_lane_stops_after_exactly_the_knob_attempts() {
    let provider = FakeProvider::new(vec![dying_turn(ProviderError::StreamClosed)]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(3),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_error_exit(&outcome, "closed before the turn finished");
    assert_eq!(provider.requests().len(), 3, "three attempts, no more");
    assert_two_retries(&events, "stream_closed");
}

#[tokio::test(start_paused = true)]
async fn connect_phase_failures_never_step_retry() {
    let provider = FakeProvider::new(vec![failing_turn("socket closed")]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(3),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_error_exit(&outcome, "socket closed");
    assert_eq!(
        provider.requests().len(),
        1,
        "stream() failures already climbed the provider ladder; the step ladder never re-retries them"
    );
    assert!(retried(&events).is_empty());
}

#[tokio::test(start_paused = true)]
async fn deterministic_in_stream_classes_fail_without_one_replay() {
    for err in [
        ProviderError::Auth {
            detail: String::from("bad key"),
        },
        ProviderError::InvalidRequest {
            detail: String::from("bad shape"),
        },
        ProviderError::ContextWindow {
            detail: String::from("too long"),
        },
        ProviderError::PayloadTooLarge {
            detail: String::from("too big"),
        },
        ProviderError::QuotaExhausted {
            detail: String::from("spent"),
        },
    ] {
        let provider = FakeProvider::new(vec![Turn::Events(vec![Err(err.clone())])]);
        let runner = Runner::new(
            provider.clone(),
            registry_with_ok("scripted", "x"),
            config(3),
        );
        let (outcome, events) = run_collect(&runner, "go").await;
        assert_eq!(outcome.exit_code(), 1, "{err:?}");
        assert_eq!(provider.requests().len(), 1, "{err:?} must not replay");
        assert!(retried(&events).is_empty(), "{err:?} must not retry");
    }
}

#[tokio::test(start_paused = true)]
async fn fold_failures_are_semantic_and_never_replay() {
    let provider = FakeProvider::new(vec![tool_turn_without_name()]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(3),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_error_exit(&outcome, "assembly failed");
    assert_eq!(provider.requests().len(), 1);
    assert!(retried(&events).is_empty());
}

#[tokio::test(start_paused = true)]
async fn empty_turns_replay_until_content_and_keep_their_usage_charged() {
    let provider = FakeProvider::new(vec![
        usage_only_turn(usage(5, 5)),
        usage_only_turn(usage(5, 5)),
        text_turn("finally", StopReason::EndTurn, usage(5, 5)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(3),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(provider.requests().len(), 3);
    assert_two_retries(&events, "empty_response");
    // Every replayed attempt's usage stays charged to the run.
    assert_usage(&outcome, 15, 15);
    assert!(events.iter().any(|event| matches!(
        event,
        RuntimeEvent::TurnUsage { usage, .. } if usage.input_tokens == 5
    )));
}

#[tokio::test(start_paused = true)]
async fn a_filtered_empty_turn_fails_without_one_replay() {
    let provider = FakeProvider::new(vec![empty_stop_turn(
        StopReason::ContentFilter,
        usage(2, 2),
    )]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(3),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_error_exit(&outcome, "empty response");
    assert_eq!(
        provider.requests().len(),
        1,
        "a filtered response is deterministic"
    );
    assert!(retried(&events).is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_budget_guard_wins_over_a_replay() {
    let provider = FakeProvider::new(vec![
        usage_only_turn(usage(30, 0)),
        usage_only_turn(usage(30, 0)),
        usage_only_turn(usage(30, 0)),
    ]);
    let mut cfg = config(3);
    cfg.max_total_tokens = Some(50);
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", "x"), cfg);
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_budget_exit(&outcome, 60, 50);
    assert_eq!(outcome.usage.input_tokens, 60);
    assert_eq!(provider.requests().len(), 2, "the third replay never fired");
    assert_single_retry(&events, "empty_response");
}

#[tokio::test(start_paused = true)]
async fn idle_stalls_replay_on_the_same_ladder() {
    let provider = FakeProvider::new(vec![
        Turn::Stall,
        Turn::Stall,
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let mut cfg = config(3);
    cfg.idle_timeout = Some(Duration::from_millis(50));
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", "x"), cfg);
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(provider.requests().len(), 3);
    assert_two_retries(&events, "idle_timeout");
}

/// A stream that ends cleanly at EOF without the terminal event is a
/// mid-stream death: the fold reports it unfinished and the ladder
/// replays the step from the intact transcript.
#[tokio::test(start_paused = true)]
async fn an_unfinished_stream_replays_on_the_step_ladder() {
    let provider = FakeProvider::new(vec![
        unfinished_turn("partial"),
        text_turn("done", StopReason::EndTurn, usage(2, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(3),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.turns, 1);
    assert_eq!(outcome.usage.input_tokens, 2);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let transcript = wire(&requests, 0);
    assert_eq!(
        wire(&requests, 1),
        transcript,
        "the replay resends the intact transcript"
    );
    assert_single_retry(&events, "stream_closed");
}

/// A tool-call header with no name: the fold fails closed at finish.
fn tool_turn_without_name() -> Turn {
    Turn::Events(vec![
        Ok(craft_core::events::ModelEvent::ToolCallDelta {
            index: 0,
            id: Some(String::from("c1")),
            name: None,
            arguments: String::from("{}"),
        }),
        Ok(craft_core::events::ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: usage(1, 1),
        }),
    ])
}
