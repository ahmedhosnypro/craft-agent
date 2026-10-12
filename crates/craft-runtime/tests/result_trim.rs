//! Result-trimming corpus: deterministic, keyless, offline.
//!
//! A scripted fake provider drives the real Runner; each test pins one
//! clause of the assembled-prompt trimming contract. Corpus index (one
//! scenario per test):
//!
//!   1  results outside the window ride as size stubs while the recent
//!      window stays verbatim — and the same result is verbatim in one
//!      request and a stub in the next
//!   2  window 0 disables trimming entirely
//!   3  the session record keeps full content while prompts carry stubs
//!   4  a resumed session trims its loaded history by transcript turn
//!   5  error results keep their flag and verbatim error text rides
//!      inside the window
//!
//! The pure knob and assemble rows live in src/trim.rs.

mod common;

use common::*;
use craft_core::types::{ChatMessage, StopReason};
use craft_runtime::{RunConfig, Runner};
use std::path::PathBuf;

// Ten bytes, so every stub in the corpus names the same size.
const PAYLOAD: &str = "0123456789";

fn config(window: u32) -> RunConfig {
    let mut cfg = RunConfig::new(PathBuf::from("."));
    cfg.step_attempts = Some(1);
    cfg.result_window_turns = Some(window);
    cfg
}

/// Distinct arguments per turn so the stationarity guard never nudges.
fn four_tool_turns_then_finish() -> Vec<Turn> {
    vec![
        tool_turn("c1", "scripted", r#"{"n":1}"#, usage(1, 1)),
        tool_turn("c2", "scripted", r#"{"n":2}"#, usage(1, 1)),
        tool_turn("c3", "scripted", r#"{"n":3}"#, usage(1, 1)),
        tool_turn("c4", "scripted", r#"{"n":4}"#, usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]
}

#[tokio::test]
async fn old_results_ride_as_stubs_while_recent_turns_stay_verbatim() {
    let provider = FakeProvider::new(four_tool_turns_then_finish());
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", PAYLOAD),
        config(2),
    );
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let requests = provider.requests();
    assert_eq!(requests.len(), 5);

    // Turn 4's request: turn 1's result is distance 2, at the window
    // edge; turns 2 and 3 ride verbatim.
    let fourth = &requests[3].messages;
    assert!(matches!(
        &fourth[2],
        ChatMessage::Tool { call_id, content, is_error: false, .. }
        if call_id == "c1" && content == "[tool result trimmed: 10 bytes, turn 1]"
    ));
    assert!(matches!(&fourth[4], ChatMessage::Tool { content, .. } if content == PAYLOAD));
    assert!(matches!(&fourth[6], ChatMessage::Tool { content, .. } if content == PAYLOAD));

    // Turn 5's request: turn 2 joins turn 1 outside the window; turns 3
    // and 4 stay verbatim.
    let fifth = &requests[4].messages;
    assert!(matches!(
        &fifth[2],
        ChatMessage::Tool { content, .. }
        if content == "[tool result trimmed: 10 bytes, turn 1]"
    ));
    assert!(matches!(
        &fifth[4],
        ChatMessage::Tool { content, .. }
        if content == "[tool result trimmed: 10 bytes, turn 2]"
    ));
    assert!(matches!(&fifth[6], ChatMessage::Tool { content, .. } if content == PAYLOAD));
    assert!(matches!(&fifth[8], ChatMessage::Tool { content, .. } if content == PAYLOAD));
}

#[tokio::test]
async fn zero_window_disables_trimming_entirely() {
    let provider = FakeProvider::new(four_tool_turns_then_finish());
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", PAYLOAD),
        config(0),
    );
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let requests = provider.requests();
    let fifth = &requests[4].messages;
    for index in [2, 4, 6, 8] {
        assert!(
            matches!(&fifth[index], ChatMessage::Tool { content, .. } if content == PAYLOAD),
            "window 0 keeps result {index} verbatim"
        );
    }
}

#[tokio::test]
async fn the_session_record_keeps_full_content_while_prompts_carry_stubs() {
    let path = session_path("trim-record");
    let mut cfg = config(2);
    cfg.session_path = Some(path.clone());
    let provider = FakeProvider::new(four_tool_turns_then_finish());
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", PAYLOAD), cfg);
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    // The record: every logged result is the full payload, in order.
    let record: Vec<ChatMessage> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let logged: Vec<&ChatMessage> = record
        .iter()
        .filter(|message| matches!(message, ChatMessage::Tool { .. }))
        .collect();
    assert_eq!(logged.len(), 4);
    assert!(
        logged.iter().all(
            |message| matches!(message, ChatMessage::Tool { content, .. } if content == PAYLOAD)
        )
    );

    // The last prompt: turn 1's result rides as the size stub.
    let fifth = &provider.requests()[4].messages;
    assert!(matches!(
        &fifth[2],
        ChatMessage::Tool { content, .. }
        if content == "[tool result trimmed: 10 bytes, turn 1]"
    ));
}

#[tokio::test]
async fn a_resumed_session_trims_its_loaded_history() {
    let path = session_path("trim-resume");
    let script = vec![
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        tool_turn("c2", "scripted", "{}", usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ];
    let mut first_cfg = config(0);
    first_cfg.session_path = Some(path.clone());
    let first = Runner::new(
        FakeProvider::new(script),
        registry_with_ok("scripted", PAYLOAD),
        first_cfg,
    );
    let (outcome, _) = run_collect(&first, "run one").await;
    assert_eq!(outcome.exit_code(), 0);

    // The resumed run measures turns across the loaded transcript:
    // turn 1's result is distance 2, turn 2's is distance 1.
    let mut second_cfg = config(2);
    second_cfg.session_path = Some(path);
    let provider = FakeProvider::new(vec![text_turn("ok", StopReason::EndTurn, usage(1, 1))]);
    let second = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", PAYLOAD),
        second_cfg,
    );
    let (outcome, _) = run_collect(&second, "run two").await;
    assert_eq!(outcome.exit_code(), 0);

    let request = &provider.requests()[0].messages;
    assert!(matches!(
        &request[2],
        ChatMessage::Tool { content, .. }
        if content == "[tool result trimmed: 10 bytes, turn 1]"
    ));
    assert!(matches!(&request[4], ChatMessage::Tool { content, .. } if content == PAYLOAD));
    assert!(matches!(
        request.last(),
        Some(ChatMessage::User { text, .. }) if text == "run two"
    ));
}

#[tokio::test]
async fn error_results_keep_their_flag_and_verbatim_text_inside_the_window() {
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", r#"{"n":1}"#, usage(1, 1)),
        tool_turn("c2", "scripted", r#"{"n":2}"#, usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let registry = registry_with_error(
        "scripted",
        craft_tools::ToolError::NotFound {
            path: String::from("nope.txt"),
        },
    );
    let runner = Runner::new(provider.clone(), registry, config(1));
    let (outcome, _) = run_collect(&runner, "fail twice").await;
    assert_eq!(outcome.exit_code(), 0);

    // Window 1 at turn 3's request: turn 1's error result rides as a
    // stub that keeps the flag; turn 2's stays verbatim.
    let third = &provider.requests()[2].messages;
    assert!(matches!(
        &third[2],
        ChatMessage::Tool { is_error: true, content, .. }
        if content.starts_with("[tool result trimmed: ") && content.ends_with(", turn 1]")
    ));
    assert!(matches!(
        &third[4],
        ChatMessage::Tool { is_error: true, content, .. } if content.contains("file not found")
    ));
}
