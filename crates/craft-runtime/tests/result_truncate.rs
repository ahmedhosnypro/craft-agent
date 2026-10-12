//! Result-truncation corpus: deterministic, keyless, offline.
//!
//! A scripted fake provider drives the real Runner; each test pins one
//! clause of the per-result truncation contract — the bound fails
//! closed on the assembled prompt while every spill write fails open.
//! Corpus index (one scenario per test):
//!
//!   1  an oversized in-window result rides head plus a pointer to a
//!      spill file carrying the full content
//!   2  the session record and the stream keep full content while
//!      prompts carry the marker
//!   3  window trimming wins: an old oversized result rides the trim
//!      stub, never a second marker, and never spills
//!   4  a result exactly at the cap rides verbatim with no spill
//!   5  a resumed session re-materializes a deleted spill tree from
//!      the record and names the new run's resolved root
//!   6  an unwritable spill root still completes the run with the
//!      no-path marker
//!   7  the budget guard still runs before completion with truncation
//!      active
//!   8  knob zero disables truncation and touches no disk
//!   9  an oversized error result keeps its flag and head
//!
//! The pure knob and cap rows live in src/truncate.rs.

mod common;

use common::*;
use craft_core::types::{ChatMessage, StopReason};
use craft_runtime::{RunConfig, Runner, RuntimeEvent};
use std::path::{Path, PathBuf};

// Cap and oversized payload fixed for the whole corpus so every
// marker names the same arithmetic.
const CAP: usize = 4096;
const BIG: usize = 8192;

fn payload(len: usize) -> String {
    "x".repeat(len)
}

/// The with-spill marker, pinned against src drift.
fn with_spill(kept: usize, total: usize, path: &Path) -> String {
    format!(
        "[tool result truncated: kept first {kept} of {total} bytes; the full result is saved at {}; read it back in ranges with bash, e.g. sed -n '1,80p' {}]",
        path.display(),
        path.display()
    )
}

/// The no-path marker for the corpus's fixed cap and payload.
const NO_SPILL: &str = "[tool result truncated: kept first 4096 of 8192 bytes; the tail is not recoverable in this run — re-derive it if needed]";

fn config(cap: u64, window: u32, root: &Path) -> RunConfig {
    let mut cfg = RunConfig::new(PathBuf::from("."));
    cfg.step_attempts = Some(1);
    cfg.result_window_turns = Some(window);
    cfg.result_truncate_bytes = Some(cap);
    cfg.spill_root = Some(root.to_path_buf());
    cfg
}

/// A fresh spill root under the workspace's ignored target/tmp,
/// unique per call; the spill tree under it is created lazily.
fn spill_root(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    let _ = std::fs::create_dir_all(&dir);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!("spill-{tag}-{}-{n}", std::process::id()))
}

/// The spill directory a session-backed run writes, from the record's
/// file stem.
fn spill_dir(root: &Path, session: &Path) -> PathBuf {
    root.join("craft-agent-spill")
        .join(session.file_stem().unwrap_or_default())
}

/// The session record's messages, verbatim and in order.
fn record_messages(path: &Path) -> Vec<ChatMessage> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The head-plus-marker prefix every capped ride must start with.
fn head_prefix() -> String {
    format!(
        "{}\n[tool result truncated: kept first {CAP} of {BIG} bytes;",
        payload(CAP)
    )
}

/// One tool turn returning the payload, then the finish.
fn tool_then_finish() -> Vec<Turn> {
    vec![
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]
}

#[tokio::test]
async fn an_oversized_in_window_result_rides_head_plus_pointer() {
    let session = session_path("trunc-pointer");
    let root = spill_root("pointer");
    let mut cfg = config(4096, 20, &root);
    cfg.session_path = Some(session.clone());
    let big = payload(BIG);
    let provider = FakeProvider::new(tool_then_finish());
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", &big), cfg);
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let path = spill_dir(&root, &session).join("result-000002.txt");
    let second = &provider.requests()[1].messages;
    assert!(matches!(
        &second[2],
        ChatMessage::Tool { call_id, content, is_error: false, .. }
        if call_id == "c1"
            && content == &format!("{}\n{}", payload(CAP), with_spill(CAP, BIG, &path))
    ));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        big,
        "the spill file carries the full result"
    );
}

#[tokio::test]
async fn the_record_and_stream_keep_full_content_while_prompts_carry_the_marker() {
    let session = session_path("trunc-record");
    let root = spill_root("record");
    let mut cfg = config(4096, 20, &root);
    cfg.session_path = Some(session.clone());
    let big = payload(BIG);
    let provider = FakeProvider::new(tool_then_finish());
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", &big), cfg);
    let (outcome, events) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let record = record_messages(&session);
    assert!(matches!(
        &record[2],
        ChatMessage::Tool { content, .. } if content == &big
    ));
    assert!(events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolResult { content, .. } if content == &big
    )));
    let second = &provider.requests()[1].messages;
    assert!(matches!(
        &second[2],
        ChatMessage::Tool { content, .. } if content.starts_with(&head_prefix())
    ));
}

#[tokio::test]
async fn window_trimming_wins_no_double_stub_no_spill_for_old_results() {
    let session = session_path("trunc-trim");
    let root = spill_root("trim");
    let big = payload(BIG);

    // Run one rides the big result verbatim: truncation off, and the
    // window wide enough that trimming never touches it.
    let mut first_cfg = config(0, 20, &root);
    first_cfg.session_path = Some(session.clone());
    let first = Runner::new(
        FakeProvider::new(tool_then_finish()),
        registry_with_ok("scripted", &big),
        first_cfg,
    );
    let (outcome, _) = run_collect(&first, "run one").await;
    assert_eq!(outcome.exit_code(), 0);

    // Run two reopens the record with window 1: the loaded result is
    // already outside the window, so the prompt carries only the trim
    // stub — never the truncated marker — and no spill is written for
    // an old result (capping trim's output, or trimming after capping,
    // would leave a file behind).
    let mut second_cfg = config(4096, 1, &root);
    second_cfg.session_path = Some(session.clone());
    let provider = FakeProvider::new(vec![text_turn("ok", StopReason::EndTurn, usage(1, 1))]);
    let second = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", &big),
        second_cfg,
    );
    let (outcome, _) = run_collect(&second, "run two").await;
    assert_eq!(outcome.exit_code(), 0);

    let request = &provider.requests()[0].messages;
    assert!(matches!(
        &request[2],
        ChatMessage::Tool { content, .. }
        if content == "[tool result trimmed: 8192 bytes, turn 1]"
    ));
    assert!(
        !root.join("craft-agent-spill").exists(),
        "an out-of-window result never spills"
    );
}

#[tokio::test]
async fn a_result_exactly_at_cap_rides_verbatim_with_no_spill() {
    let session = session_path("trunc-atcap");
    let root = spill_root("atcap");
    let mut cfg = config(4096, 20, &root);
    cfg.session_path = Some(session.clone());
    let exact = payload(CAP);
    let provider = FakeProvider::new(tool_then_finish());
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", &exact), cfg);
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let second = &provider.requests()[1].messages;
    assert!(matches!(
        &second[2],
        ChatMessage::Tool { content, .. } if content == &exact
    ));
    assert!(!root.join("craft-agent-spill").exists());
}

#[tokio::test]
async fn a_resumed_session_rematerializes_missing_spills() {
    let session = session_path("trunc-resume");
    let first_root = spill_root("resume-a");
    let second_root = spill_root("resume-b");
    let big = payload(BIG);

    let mut first_cfg = config(4096, 20, &first_root);
    first_cfg.session_path = Some(session.clone());
    let first = Runner::new(
        FakeProvider::new(tool_then_finish()),
        registry_with_ok("scripted", &big),
        first_cfg,
    );
    let (outcome, _) = run_collect(&first, "run one").await;
    assert_eq!(outcome.exit_code(), 0);
    let spilled = spill_dir(&first_root, &session).join("result-000002.txt");
    assert!(spilled.exists());
    std::fs::remove_dir_all(first_root.join("craft-agent-spill")).unwrap();

    let mut second_cfg = config(4096, 20, &second_root);
    second_cfg.session_path = Some(session.clone());
    let provider = FakeProvider::new(vec![text_turn("ok", StopReason::EndTurn, usage(1, 1))]);
    let second = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", &big),
        second_cfg,
    );
    let (outcome, _) = run_collect(&second, "run two").await;
    assert_eq!(outcome.exit_code(), 0);

    let path = spill_dir(&second_root, &session).join("result-000002.txt");
    let request = &provider.requests()[0].messages;
    assert!(matches!(
        &request[2],
        ChatMessage::Tool { content, .. }
        if content == &format!("{}\n{}", payload(CAP), with_spill(CAP, BIG, &path))
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), big);

    let record = record_messages(&session);
    assert!(matches!(
        &record[2],
        ChatMessage::Tool { content, .. } if content == &big
    ));
}

#[tokio::test]
async fn an_unwritable_spill_root_still_completes_the_run() {
    let blocker = spill_root("enotdir");
    std::fs::write(&blocker, b"blocked").unwrap();
    let root = blocker.join("spill");
    let cfg = config(4096, 20, &root);
    let big = payload(BIG);
    let provider = FakeProvider::new(tool_then_finish());
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", &big), cfg);
    let (outcome, events) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let second = &provider.requests()[1].messages;
    assert!(matches!(
        &second[2],
        ChatMessage::Tool { content, .. }
        if content == &format!("{}\n{NO_SPILL}", payload(CAP))
    ));
    assert!(events.iter().any(|event| matches!(
        event,
        RuntimeEvent::ToolResult { content, .. } if content == &big
    )));
}

#[tokio::test]
async fn the_budget_guard_still_runs_before_completion() {
    let root = spill_root("budget");
    let mut cfg = config(4096, 20, &root);
    cfg.max_total_tokens = 100;
    let big = payload(BIG);
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", usage(60, 0)),
        text_turn("done", StopReason::EndTurn, usage(30, 15)),
    ]);
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", &big), cfg);
    let (outcome, _) = run_collect(&runner, "spend").await;
    assert_eq!(outcome.exit_code(), 3);
    assert!(
        outcome
            .detail
            .contains("token budget exceeded: 105 tokens spent, 100 allowed"),
        "{outcome:?}"
    );
    let second = &provider.requests()[1].messages;
    assert!(matches!(
        &second[2],
        ChatMessage::Tool { content, .. } if content.starts_with(&head_prefix())
    ));
}

#[tokio::test]
async fn knob_zero_disables_truncation_and_touches_no_disk() {
    let root = spill_root("knob-zero");
    let cfg = config(0, 20, &root);
    let big = payload(BIG);
    let provider = FakeProvider::new(tool_then_finish());
    let runner = Runner::new(provider.clone(), registry_with_ok("scripted", &big), cfg);
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let second = &provider.requests()[1].messages;
    assert!(matches!(
        &second[2],
        ChatMessage::Tool { content, .. } if content == &big
    ));
    assert!(
        !root.join("craft-agent-spill").exists(),
        "truncation off never touches disk"
    );
}

#[tokio::test]
async fn an_oversized_error_result_keeps_its_flag_and_head() {
    let session = session_path("trunc-error");
    let root = spill_root("error");
    let mut cfg = config(4096, 20, &root);
    cfg.session_path = Some(session.clone());
    let error_text = format!("command exited Some(1): {}", payload(BIG));
    let provider = FakeProvider::new(tool_then_finish());
    let registry = registry_with_error(
        "scripted",
        craft_tools::ToolError::NonZeroExit {
            code: Some(1),
            stderr: payload(BIG),
        },
    );
    let runner = Runner::new(provider.clone(), registry, cfg);
    let (outcome, _) = run_collect(&runner, "fail").await;
    assert_eq!(outcome.exit_code(), 0);

    let second = &provider.requests()[1].messages;
    assert!(matches!(
        &second[2],
        ChatMessage::Tool { is_error: true, content, .. }
        if content.starts_with(&error_text[..CAP])
            && content.contains("kept first 4096 of 8216 bytes")
    ));

    let record = record_messages(&session);
    assert!(matches!(
        &record[2],
        ChatMessage::Tool { is_error: true, content, .. } if content == &error_text
    ));
}
