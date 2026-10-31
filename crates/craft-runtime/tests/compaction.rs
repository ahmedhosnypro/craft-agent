//! Auto-compaction corpus: the trigger, the history swap, the typed
//! `Compacted` event, the summary-call cap, the attempt bound, and
//! fail-soft behavior on a dead summary call.

mod common;

use common::*;
use craft_core::types::{ChatMessage, ModelRequest, StopReason};
use craft_runtime::{RunConfig, RunOutcome, Runner, RuntimeEvent};

/// A config with a context cap and otherwise production defaults.
fn capped(max_context: u64) -> RunConfig {
    let mut cfg = RunConfig::new(std::path::PathBuf::from("."));
    cfg.max_context_tokens = Some(max_context);
    cfg
}

/// A context cap of 1M with the input-side trigger at 100k: 90k used
/// is past 85% of the input limit while the 50k reserve stays far from
/// the context cap, so the ratio arm alone fires.
fn ratio_capped() -> RunConfig {
    let mut cfg = capped(1_000_000);
    cfg.max_input_tokens = Some(100_000);
    cfg
}

/// Runs one scripted row to completion, keeping the provider handle so
/// the test can inspect every request the run sent.
async fn run_script(
    script: Vec<Turn>,
    cfg: RunConfig,
) -> (RunOutcome, Vec<RuntimeEvent>, std::sync::Arc<FakeProvider>) {
    let provider = FakeProvider::new(script);
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", "out"), cfg);
    let (outcome, events) = run_collect(&runner, "start the work").await;
    (outcome, events, provider)
}

/// The user text of a message, when it is a user message.
fn user_text(message: &std::sync::Arc<ChatMessage>) -> Option<&str> {
    match message.as_ref() {
        ChatMessage::User { text, .. } => Some(text),
        _ => None,
    }
}

/// The requests that were compaction summary calls: no tools offered
/// and the compaction instruction as the last user message.
fn compaction_requests(provider: &FakeProvider) -> Vec<ModelRequest> {
    provider
        .requests()
        .into_iter()
        .filter(|request| {
            request.tools.is_empty()
                && request
                    .messages
                    .last()
                    .and_then(user_text)
                    .is_some_and(|text| text.contains("compact brief"))
        })
        .collect()
}

/// The `Compacted` events of a run.
fn compacted_events(events: &[RuntimeEvent]) -> Vec<(u32, u32, usize, usize)> {
    events
        .iter()
        .filter_map(|event| match event {
            RuntimeEvent::Compacted {
                turn,
                attempt,
                kept_messages,
                summary_bytes,
            } => Some((*turn, *attempt, *kept_messages, *summary_bytes)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn trigger_swaps_history_and_emits_compacted() {
    // Turn one reports 90k input tokens: past 85% of the 100k input
    // limit, so before turn two the runner makes one summary call and
    // swaps the history prefix.
    let (outcome, events, provider) = run_script(
        vec![
            tool_turn("c1", "scripted", "{}", usage(90_000, 10)),
            text_turn("the summary", StopReason::EndTurn, usage(20, 5)),
            text_turn("done", StopReason::EndTurn, usage(10, 1)),
        ],
        ratio_capped(),
    )
    .await;

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(
        compacted_events(&events),
        vec![(2, 1, 3, "the summary".len())],
        "one swap before turn two: the tail of three messages kept, summary byte count"
    );

    let requests = provider.requests();
    assert_eq!(requests.len(), 3, "turn, summary call, turn");

    let summary_call = &requests[1];
    assert!(
        summary_call.tools.is_empty(),
        "the summary call offers no tools"
    );
    assert_eq!(
        summary_call.max_tokens, 8192,
        "min(default output cap, ceiling)"
    );
    assert_eq!(
        summary_call.messages.len(),
        4,
        "three session messages plus the instruction"
    );

    let final_request = &requests[2];
    assert_eq!(
        final_request.messages.len(),
        4,
        "summary plus the kept tail"
    );
    let summary = user_text(&final_request.messages[0]).unwrap_or("not a user message");
    assert!(
        summary.starts_with("[compacted summary] ") && summary.contains("the summary"),
        "the prefix became the summary message: {summary}"
    );
    // The tail rides verbatim: prompt, assistant tool call, tool result.
    assert!(user_text(&final_request.messages[1]).is_some_and(|t| t == "start the work"));
    assert!(matches!(
        final_request.messages[2].as_ref(),
        ChatMessage::Assistant { .. }
    ));
    assert!(matches!(
        final_request.messages[3].as_ref(),
        ChatMessage::Tool { .. }
    ));
}

#[tokio::test]
async fn the_summary_call_cap_is_the_min_of_output_cap_and_ceiling() {
    for (output_cap, expected) in [(200_000u32, 131_072u32), (64, 64)] {
        let mut cfg = ratio_capped();
        cfg.max_tokens = output_cap;
        let (_, _, provider) = run_script(
            vec![
                tool_turn("c1", "scripted", "{}", usage(90_000, 10)),
                text_turn("s", StopReason::EndTurn, usage(1, 1)),
                text_turn("done", StopReason::EndTurn, usage(1, 1)),
            ],
            cfg,
        )
        .await;
        let calls = compaction_requests(&provider);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].max_tokens, expected, "output cap {output_cap}");
    }
}

#[tokio::test]
async fn failed_summary_calls_count_attempts_and_the_run_continues() {
    // Every summary call dies; each counts one attempt and the run
    // continues. Past the third attempt compaction stops for the rest
    // of the run, and the fourth turn sees the full history.
    let (outcome, events, provider) = run_script(
        vec![
            tool_turn("c1", "scripted", "{}", usage(90_000, 10)),
            failing_turn("summary backend dead"),
            tool_turn("c2", "scripted", "{}", usage(90_000, 10)),
            failing_turn("summary backend dead"),
            tool_turn("c3", "scripted", "{}", usage(90_000, 10)),
            failing_turn("summary backend dead"),
            text_turn("done", StopReason::EndTurn, usage(10, 1)),
        ],
        ratio_capped(),
    )
    .await;

    assert_eq!(outcome.exit_code(), 0, "fail-soft: {outcome:?}");
    assert!(
        compacted_events(&events).is_empty(),
        "a failed summary call emits no Compacted event"
    );
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        7,
        "three turns, three summary calls, final turn"
    );
    assert_eq!(
        compaction_requests(&provider).len(),
        3,
        "the attempt bound is three"
    );
    assert_eq!(
        requests[6].messages.len(),
        7,
        "no swap happened: prompt plus three assistant/tool pairs"
    );
}

#[tokio::test]
async fn no_compaction_without_a_context_cap_or_below_the_trigger() {
    // No cap set: huge reported usage never triggers a summary call.
    let script = || {
        vec![
            tool_turn("c1", "scripted", "{}", usage(999_999, 1)),
            text_turn("done", StopReason::EndTurn, usage(1, 1)),
        ]
    };
    let (outcome, events, provider) =
        run_script(script(), RunConfig::new(std::path::PathBuf::from("."))).await;
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(provider.requests().len(), 2);
    assert!(compacted_events(&events).is_empty());

    // Cap set but usage far under both trigger arms.
    let (outcome, events, provider) = run_script(
        vec![
            tool_turn("c1", "scripted", "{}", usage(900, 1)),
            text_turn("done", StopReason::EndTurn, usage(1, 1)),
        ],
        capped(1_000_000),
    )
    .await;
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(provider.requests().len(), 2);
    assert!(compacted_events(&events).is_empty());
}

#[tokio::test]
async fn max_input_overrides_the_ratio_trigger() {
    // Context cap 1M but input limit 1000: 900 used is past 85% of the
    // input limit, so compaction fires despite the distant context cap.
    let mut cfg = capped(1_000_000);
    cfg.max_input_tokens = Some(1000);
    let (outcome, events, provider) = run_script(
        vec![
            tool_turn("c1", "scripted", "{}", usage(900, 10)),
            text_turn("s", StopReason::EndTurn, usage(1, 1)),
            text_turn("done", StopReason::EndTurn, usage(1, 1)),
        ],
        cfg,
    )
    .await;
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(compacted_events(&events).len(), 1);
    assert_eq!(compaction_requests(&provider).len(), 1);
}
