//! The patience window: a transient stream death rides past the step's
//! attempt bound while the window is open, and stops when it closes.
//! Paused tokio time keeps every backoff instant; the knob parse rows
//! live in step_retry_knob.rs.

mod common;

use common::*;
use craft_core::error::ProviderError;
use craft_core::types::StopReason;
use craft_runtime::{CancelCategory, RunConfig, Runner};
use std::path::PathBuf;
use std::time::Duration;

fn config(attempts: u32, patience: Duration) -> RunConfig {
    let mut config = RunConfig::new(PathBuf::from("."));
    config.step_attempts = Some(attempts);
    config.step_patience = Some(patience);
    config
}

fn reasons(events: &[craft_runtime::RuntimeEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            craft_runtime::RuntimeEvent::TurnRetried { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_transient_death_rides_past_the_attempt_bound_while_patience_lasts() {
    let dead = ProviderError::StreamClosed;
    let provider = FakeProvider::new(vec![
        dying_turn(dead.clone()),
        dying_turn(dead),
        text_turn("done", StopReason::EndTurn, usage(2, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(1, Duration::from_secs(5)),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(
        provider.requests().len(),
        3,
        "three attempts under an attempt bound of one"
    );
    assert_eq!(reasons(&events), vec!["stream_closed", "stream_closed"]);
}

#[tokio::test(start_paused = true)]
async fn an_always_dying_lane_stops_when_the_patience_closes() {
    let provider = FakeProvider::new(vec![dying_turn(ProviderError::StreamClosed)]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(1, Duration::from_secs(2)),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 1);
    // The ladder's backoffs (0.5s, 1s, 2s, jittered downward by at most
    // 25%) put the fourth attempt past the 2s window, so exactly four
    // attempts run under an attempt bound of one.
    assert_eq!(provider.requests().len(), 4, "patience bounds the retries");
    assert_eq!(reasons(&events).len(), 3);
}

#[tokio::test(start_paused = true)]
async fn a_deterministic_failure_fails_even_with_patience_open() {
    let provider = FakeProvider::new(vec![Turn::Fail(ProviderError::Auth {
        detail: String::from("bad key"),
    })]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(1, Duration::from_secs(60)),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(provider.requests().len(), 1, "auth never replays");
    assert!(reasons(&events).is_empty());
}

#[tokio::test(start_paused = true)]
async fn zero_patience_restores_the_attempt_bound_alone() {
    let provider = FakeProvider::new(vec![dying_turn(ProviderError::StreamClosed)]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(2, Duration::ZERO),
    );
    let (outcome, events) = run_collect(&runner, "go").await;

    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(provider.requests().len(), 2);
    assert_eq!(reasons(&events).len(), 1);
    assert!(matches!(
        events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished {
            category: CancelCategory::Error,
            ..
        })
    ));
}
