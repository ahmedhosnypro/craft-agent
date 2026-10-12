//! Journal corpus: dense seq, torn-tail repair, hostile files, caps, and
//! divergence detection through resume.
#![allow(clippy::unwrap_used)]
mod common;

use common::{FakeExecutor, journal_path, no_args, temp_dir};
use craft_workflow::{
    Journal, JournalEvent, SkipReason, WorkflowError, WorkflowPlan, resume_workflow, run_workflow,
};

fn plan(text: &str) -> WorkflowPlan {
    WorkflowPlan::parse("t.md", text).unwrap()
}

const ONE_STEP: &str =
    "---\nname: one\nsteps:\n  - id: a\n    agent: w\n    body: |\n      a\n---\nprelude\n";

async fn run_one(dir: &std::path::Path, fake: &FakeExecutor) -> Journal {
    let mut journal = Journal::open(&journal_path(dir)).unwrap();
    run_workflow(&plan(ONE_STEP), &no_args(), fake, &mut journal)
        .await
        .unwrap();
    journal
}

fn file_bytes(dir: &std::path::Path) -> Vec<u8> {
    std::fs::read(journal_path(dir)).unwrap()
}

fn rewrite(dir: &std::path::Path, bytes: &[u8]) {
    std::fs::write(journal_path(dir), bytes).unwrap();
}

#[tokio::test]
async fn record_and_replay_roundtrip() {
    let dir = temp_dir("roundtrip");
    let journal = run_one(&dir, &FakeExecutor::new()).await;
    let reloaded = Journal::open(&journal_path(&dir)).unwrap();
    assert_eq!(reloaded.len(), journal.len());
    for (i, (live, reread)) in journal.entries().iter().zip(reloaded.entries()).enumerate() {
        assert_eq!(live.seq(), i as u64);
        assert_eq!(live, reread);
    }
    assert!(matches!(
        reloaded.entries()[0],
        JournalEvent::RunStarted { .. }
    ));
    assert!(matches!(
        reloaded.entries().last(),
        Some(JournalEvent::RunFinished { .. })
    ));
}

#[tokio::test]
async fn torn_tail_is_truncated_to_the_valid_prefix() {
    let dir = temp_dir("torn");
    let journal = run_one(&dir, &FakeExecutor::new()).await;
    let valid = file_bytes(&dir);
    let mut damaged = valid.clone();
    damaged.extend_from_slice(b"{\"type\":\"step_sta");
    rewrite(&dir, &damaged);
    let repaired = Journal::open(&journal_path(&dir)).unwrap();
    assert_eq!(repaired.len(), journal.len());
    assert_eq!(file_bytes(&dir), valid);
    let mut repaired = repaired;
    repaired
        .record(JournalEvent::StepSkipped {
            seq: 4,
            id: "x".into(),
            reason: SkipReason::Unreachable,
        })
        .unwrap();
    assert_eq!(repaired.len(), 5);
}

#[tokio::test]
async fn valid_unterminated_tail_is_kept_and_terminated() {
    let dir = temp_dir("unterm");
    let journal = run_one(&dir, &FakeExecutor::new()).await;
    let valid = file_bytes(&dir);
    assert_eq!(valid.last(), Some(&b'\n'));
    let mut unterminated = valid.clone();
    unterminated.pop();
    rewrite(&dir, &unterminated);
    let reloaded = Journal::open(&journal_path(&dir)).unwrap();
    assert_eq!(reloaded.len(), journal.len());
    assert_eq!(file_bytes(&dir), valid);
}

#[tokio::test]
async fn complete_malformed_line_is_a_hard_error() {
    let dir = temp_dir("malformed");
    run_one(&dir, &FakeExecutor::new()).await;
    let mut damaged = file_bytes(&dir);
    damaged.extend_from_slice(b"garbage\n");
    rewrite(&dir, &damaged);
    match Journal::open(&journal_path(&dir)).unwrap_err() {
        WorkflowError::JournalParse { line, .. } => assert_eq!(line, 5),
        other => panic!("expected a parse error, got {other:?}"),
    }
}

#[tokio::test]
async fn blank_line_is_a_hard_error() {
    let dir = temp_dir("blank");
    run_one(&dir, &FakeExecutor::new()).await;
    let mut damaged = file_bytes(&dir);
    damaged.extend_from_slice(b"\n");
    rewrite(&dir, &damaged);
    assert!(matches!(
        Journal::open(&journal_path(&dir)),
        Err(WorkflowError::JournalParse { .. })
    ));
}

#[tokio::test]
async fn dense_seq_is_required() {
    let dir = temp_dir("dense");
    run_one(&dir, &FakeExecutor::new()).await;
    let text = String::from_utf8(file_bytes(&dir)).unwrap();
    let tampered = text.replace("\"seq\":2,", "\"seq\":9,");
    assert_ne!(text, tampered);
    rewrite(&dir, tampered.as_bytes());
    match Journal::open(&journal_path(&dir)).unwrap_err() {
        WorkflowError::JournalSequence {
            expected: 2,
            actual: 9,
            ..
        } => {}
        other => panic!("expected a sequence error, got {other:?}"),
    }
}

#[test]
fn symlink_and_irregular_files_are_refused() {
    let dir = temp_dir("hostile");
    let real = dir.join("real.jsonl");
    std::fs::write(&real, b"\n").unwrap();
    let link = dir.join("link.jsonl");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(matches!(
        Journal::open(&link),
        Err(WorkflowError::JournalUnsafe { .. })
    ));
    let folder = dir.join("dir.jsonl");
    std::fs::create_dir(&folder).unwrap();
    assert!(matches!(
        Journal::open(&folder),
        Err(WorkflowError::JournalUnsafe { .. })
    ));
}

#[test]
fn oversize_files_are_refused_before_reading() {
    let dir = temp_dir("oversize");
    let path = journal_path(&dir);
    std::fs::write(&path, [b'x'; 100]).unwrap();
    assert!(matches!(
        Journal::open_bounded(&path, 50, 10),
        Err(WorkflowError::JournalCap { limit: 50, .. })
    ));
}

#[test]
fn record_refuses_the_entry_and_byte_caps() {
    let dir = temp_dir("caps");
    let path = journal_path(&dir);
    let event = |seq: u64| JournalEvent::StepSkipped {
        seq,
        id: "x".to_string(),
        reason: SkipReason::Unreachable,
    };
    let mut journal = Journal::open_bounded(&path, 64 * 1024, 2).unwrap();
    journal.record(event(0)).unwrap();
    journal.record(event(1)).unwrap();
    assert!(matches!(
        journal.record(event(2)),
        Err(WorkflowError::JournalFull {
            seq: 2,
            limit: 2,
            ..
        })
    ));
    let mut journal = Journal::open_bounded(&dir.join("bytes.jsonl"), 10, 10).unwrap();
    assert!(matches!(
        journal.record(event(0)),
        Err(WorkflowError::JournalCap { .. })
    ));
}

#[test]
fn persistence_failure_does_not_advance_memory() {
    let dir = temp_dir("nofail");
    let path = journal_path(&dir);
    let mut journal = Journal::open(&path).unwrap();
    let event = |seq: u64| JournalEvent::StepSkipped {
        seq,
        id: "x".to_string(),
        reason: SkipReason::Unreachable,
    };
    journal.record(event(0)).unwrap();
    journal.record(event(1)).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        journal.record(event(2)),
        Err(WorkflowError::JournalIo { .. })
    ));
    assert_eq!(journal.len(), 2);
    assert_eq!(journal.next_seq(), 2);
}

#[tokio::test]
async fn first_event_must_be_run_started() {
    let dir = temp_dir("nohead");
    rewrite(
        &dir,
        b"{\"type\":\"step_started\",\"seq\":0,\"id\":\"a\"}\n",
    );
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    let fake = FakeExecutor::new();
    match resume_workflow(&plan(ONE_STEP), &no_args(), &fake, &mut journal).await {
        Err(WorkflowError::JournalParse { line: 1, .. }) => {}
        other => panic!("expected a first-line parse error, got {other:?}"),
    }
}
