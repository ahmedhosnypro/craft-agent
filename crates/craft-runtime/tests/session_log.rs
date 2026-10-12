//! Session corpus: append-only JSONL, model-visible ⟺ logged, verbatim
//! native-block replay, and typed errors on corrupt lines.

mod common;

use common::*;
use craft_core::types::{ChatMessage, NativeBlocks, NativeDialect, StopReason};
use craft_runtime::session::{Session, SessionError};
use craft_runtime::{RunConfig, Runner};
use std::path::PathBuf;

fn config(session: PathBuf) -> RunConfig {
    let mut cfg = RunConfig::new(PathBuf::from("."));
    cfg.session_path = Some(session);
    cfg
}

fn lines(path: &PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

#[tokio::test]
async fn every_model_visible_message_lands_in_the_log() {
    let path = session_path("visible");
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "out"),
        config(path.clone()),
    );
    let (outcome, _) = run_collect(&runner, "work").await;
    assert_eq!(outcome.exit_code(), 0);

    let logged: Vec<ChatMessage> = lines(&path)
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(logged.len(), 4);
    assert!(matches!(&logged[0], ChatMessage::User { text, .. } if text == "work"));
    assert!(
        matches!(&logged[1], ChatMessage::Assistant { tool_calls, .. } if tool_calls.len() == 1)
    );
    assert!(matches!(&logged[2], ChatMessage::Tool { call_id, .. } if call_id == "c1"));
    assert!(matches!(&logged[3], ChatMessage::Assistant { text, .. } if text == "done"));
    // Every message the model saw is logged, in order; the final
    // assistant answer lands in the log even though no later request
    // ever carries it.
    assert_eq!(provider.requests()[1].messages, logged[..3]);
}

#[tokio::test]
async fn second_run_appends_without_truncating() {
    let path = session_path("append");
    let script = vec![
        tool_turn("c1", "scripted", "{}", usage(1, 1)),
        text_turn("done", StopReason::EndTurn, usage(1, 1)),
    ];
    let first = Runner::new(
        FakeProvider::new(script.clone()),
        registry_with_ok("scripted", "out"),
        config(path.clone()),
    );
    let (_, _) = run_collect(&first, "run one").await;
    let after_first = lines(&path);
    assert_eq!(after_first.len(), 4);

    let second = Runner::new(
        FakeProvider::new(vec![text_turn("ok", StopReason::EndTurn, usage(1, 1))]),
        registry_with_ok("scripted", "out"),
        config(path.clone()),
    );
    let (outcome, _) = run_collect(&second, "run two").await;
    assert_eq!(outcome.exit_code(), 0);
    let after_second = lines(&path);
    assert!(after_second.len() > after_first.len());
    assert_eq!(&after_second[..after_first.len()], &after_first[..]);
}

#[tokio::test]
async fn native_blocks_replay_into_the_next_request_verbatim() {
    let path = session_path("native");
    let seeded = vec![
        ChatMessage::User {
            text: String::from("think first"),
            images: Vec::new(),
        },
        ChatMessage::Assistant {
            text: String::from(""),
            tool_calls: Vec::new(),
            native_blocks: Some(NativeBlocks {
                dialect: NativeDialect::Anthropic,
                blocks: serde_json::json!([
                    {"type": "thinking", "thinking": "hidden reasoning", "signature": "sig-1"}
                ]),
            }),
        },
    ];
    let mut file = String::new();
    for message in &seeded {
        file.push_str(&serde_json::to_string(message).unwrap());
        file.push('\n');
    }
    std::fs::write(&path, file).unwrap();

    let provider = FakeProvider::new(vec![text_turn("done", StopReason::EndTurn, usage(1, 1))]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "out"),
        config(path),
    );
    let (outcome, _) = run_collect(&runner, "continue").await;
    assert_eq!(outcome.exit_code(), 0);

    let request = &provider.requests()[0];
    assert_eq!(request.messages[..2], seeded[..]);
}

#[tokio::test]
async fn corrupt_line_is_a_typed_error_never_a_panic() {
    let path = session_path("corrupt");
    let good = serde_json::to_string(&ChatMessage::User {
        text: String::from("fine"),
        images: Vec::new(),
    })
    .unwrap();
    std::fs::write(&path, format!("{good}\nnot json at all\n")).unwrap();

    let loaded = Session::load(&path);
    assert!(matches!(
        loaded,
        Err(SessionError::CorruptLine { line: 2, .. })
    ));

    let provider = FakeProvider::new(vec![text_turn("done", StopReason::EndTurn, usage(1, 1))]);
    let runner = Runner::new(provider, registry_with_ok("scripted", "out"), config(path));
    let (outcome, _) = run_collect(&runner, "resume").await;
    assert_eq!(outcome.exit_code(), 1);
    assert!(outcome.detail.contains("session"));
}

#[tokio::test]
async fn truncated_final_line_is_a_typed_error() {
    let path = session_path("truncated");
    let good = serde_json::to_string(&ChatMessage::User {
        text: String::from("fine"),
        images: Vec::new(),
    })
    .unwrap();
    let cut: String = good.chars().take(good.len() / 2).collect();
    std::fs::write(&path, format!("{good}\n{cut}")).unwrap();

    assert!(matches!(
        Session::load(&path),
        Err(SessionError::CorruptLine { line: 2, .. })
    ));
}

#[tokio::test]
async fn a_budget_exit_answers_pending_calls_so_the_record_resumes() {
    let path = session_path("budget-resume");
    let provider = FakeProvider::new(vec![tool_turn("c1", "scripted", "{}", usage(15, 10))]);
    let mut cfg = config(path.clone());
    cfg.max_total_tokens = 20;
    let runner = Runner::new(provider, registry_with_ok("scripted", "out"), cfg);
    let (outcome, _) = run_collect(&runner, "spend").await;
    assert_eq!(outcome.exit_code(), 3);

    let record: Vec<ChatMessage> = lines(&path)
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(matches!(
        record.last(),
        Some(ChatMessage::Tool {
            call_id,
            is_error: true,
            content,
            ..
        }) if call_id == "c1"
            && content.contains("tool call not completed: token budget exceeded")
    ));

    let provider = FakeProvider::new(vec![text_turn("done", StopReason::EndTurn, usage(1, 1))]);
    let runner = Runner::new(
        provider.clone(),
        registry_with_ok("scripted", "x"),
        config(path),
    );
    let (outcome, _) = run_collect(&runner, "resume").await;
    assert_eq!(outcome.exit_code(), 0);
    let request = &provider.requests()[0];
    assert_eq!(request.messages[..record.len()], record[..]);
    assert!(matches!(
        request.messages.last(),
        Some(ChatMessage::User { text, .. }) if text == "resume"
    ));
}

#[tokio::test]
async fn torn_tail_without_newline_is_a_typed_error() {
    let path = session_path("torn");
    let good = serde_json::to_string(&ChatMessage::User {
        text: String::from("fine"),
        images: Vec::new(),
    })
    .unwrap();
    std::fs::write(&path, good).unwrap();

    assert!(matches!(
        Session::load(&path),
        Err(SessionError::CorruptLine { line: 1, .. })
    ));
    let loaded = Session::open(path);
    assert!(matches!(
        loaded,
        Err(SessionError::CorruptLine { line: 1, .. })
    ));
}
