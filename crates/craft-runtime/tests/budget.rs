//! Budget corpus: max-turns and max-total-tokens aborts, both fail-closed
//! on unverifiable usage. Spent means input + output only — the cache
//! and reasoning buckets are subsets of those two on the provider
//! wires and are not summed into the spend.

mod common;

use common::*;
use craft_core::types::{StopReason, Usage};
use craft_runtime::{CancelCategory, RunConfig, Runner};

fn limited(max_turns: u32, max_total_tokens: u64) -> RunConfig {
    let mut cfg = RunConfig::new(std::path::PathBuf::from("."));
    cfg.max_turns = max_turns;
    cfg.max_total_tokens = max_total_tokens;
    cfg
}

#[tokio::test]
async fn max_turns_abort_is_exit_two_with_category() {
    let provider = FakeProvider::new(vec![tool_turn("c1", "scripted", "{}", usage(1, 1))]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(2, u64::MAX),
    );
    let (outcome, events) = run_collect(&runner, "loop forever").await;

    assert_eq!(outcome.exit_code(), 2);
    assert_eq!(outcome.category, CancelCategory::MaxTurns);
    assert_eq!(outcome.turns, 2);
    assert!(outcome.detail.contains("max turns"));
    assert!(matches!(
        events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished { exit_code, category, .. })
        if *exit_code == 2 && *category == CancelCategory::MaxTurns
    ));
}

#[tokio::test]
async fn max_total_tokens_abort_is_exit_three_before_more_tools() {
    let provider = FakeProvider::new(vec![tool_turn("c1", "scripted", "{}", usage(15, 10))]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 20),
    );
    let (outcome, events) = run_collect(&runner, "spend").await;

    assert_eq!(outcome.exit_code(), 3);
    assert_eq!(outcome.category, CancelCategory::Budget);
    assert!(
        outcome
            .detail
            .contains("token budget exceeded: 25 tokens spent, 20 allowed"),
        "the guard fires at the input-plus-output sum: {outcome:?}"
    );
    // The budget fired before the next tool round could execute.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, craft_runtime::RuntimeEvent::ToolResult { .. }))
    );
}

#[tokio::test]
async fn cache_read_tokens_do_not_inflate_spent() {
    // Two turns at 10 in + 5 out with a 1M-token cached prefix each:
    // the cache bucket rides inside input on the wire, so spent is
    // the 30 top-level tokens, not 2,000,030.
    let used = bucketed_usage(10, 5, 1_000_000, 0);
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", used),
        text_turn("done", StopReason::EndTurn, used),
    ]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 100),
    );
    let (outcome, _) = run_collect(&runner, "cached prefix").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.usage.input_tokens, 20);
    assert_eq!(outcome.usage.output_tokens, 10);
    assert_eq!(
        outcome.usage.cache_read_tokens, 2_000_000,
        "the cache bucket still accumulates for reporting"
    );
}

#[tokio::test]
async fn reasoning_inside_output_is_not_summed_again() {
    // 10 in + 20 out with reasoning a 20-token subset of the output,
    // on two tool turns: spent is 60, not 120, so a 61 budget funds
    // both rounds.
    let used = bucketed_usage(10, 20, 0, 20);
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", used),
        tool_turn("c2", "scripted", "{}", used),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 61),
    );
    let (outcome, _) = run_collect(&runner, "reasoning").await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.turns, 3);
    assert_eq!(outcome.usage.reasoning_tokens, 40);
}

#[tokio::test]
async fn the_guard_fires_on_input_plus_output_despite_subset_buckets() {
    // 15 in + 10 out with cache and reasoning set: the detail pins the
    // spend at input plus output, with the subsets uncounted.
    let used = bucketed_usage(15, 10, 5_000, 23);
    let provider = FakeProvider::new(vec![tool_turn("c1", "scripted", "{}", used)]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 20),
    );
    let (outcome, _) = run_collect(&runner, "spend").await;

    assert_eq!(outcome.exit_code(), 3);
    assert_eq!(outcome.category, CancelCategory::Budget);
    assert!(
        outcome
            .detail
            .contains("token budget exceeded: 25 tokens spent, 20 allowed"),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn finishing_exactly_at_budget_succeeds() {
    let provider = FakeProvider::new(vec![text_turn("done", StopReason::EndTurn, usage(10, 0))]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 10),
    );
    let (outcome, _) = run_collect(&runner, "finish").await;
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(outcome.usage.input_tokens, 10);
}

#[tokio::test]
async fn overspend_on_a_finishing_turn_still_returns_the_answer() {
    let provider = FakeProvider::new(vec![text_turn("done", StopReason::EndTurn, usage(11, 0))]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 10),
    );
    let (outcome, _) = run_collect(&runner, "finish").await;
    assert_eq!(outcome.exit_code(), 0);
}

#[tokio::test]
async fn incomplete_usage_never_silently_passes_the_budget_check() {
    let provider = FakeProvider::new(vec![tool_turn("c1", "scripted", "{}", Usage::omitted())]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 4_000_000),
    );
    let (outcome, _) = run_collect(&runner, "unverifiable").await;

    assert_eq!(outcome.exit_code(), 3);
    assert_eq!(outcome.category, CancelCategory::Budget);
    assert!(outcome.detail.contains("incomplete"));
}

#[tokio::test]
async fn incomplete_usage_on_a_finishing_turn_flags_but_completes() {
    let provider = FakeProvider::new(vec![text_turn(
        "done",
        StopReason::EndTurn,
        Usage::omitted(),
    )]);
    let runner = Runner::new(
        provider,
        registry_with_ok("scripted", "out"),
        limited(40, 4_000_000),
    );
    let (outcome, _) = run_collect(&runner, "finish").await;

    assert_eq!(outcome.exit_code(), 0);
    assert!(outcome.usage.incomplete);
}
