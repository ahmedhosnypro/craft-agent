//! Divergence and corruption corpus: a journal that no longer matches the
//! plan, args, or its own step rows is a typed divergence — never silent.
#![allow(clippy::unwrap_used)]

mod common;

use common::{FakeExecutor, journal_path, no_args, plan_texts, temp_dir};
use craft_workflow::{Journal, WorkflowError, WorkflowPlan, resume_workflow, run_workflow};

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
async fn edited_plan_diverges_on_resume() {
    let dir = temp_dir("divplan");
    let mut journal = run_one(&dir, &FakeExecutor::new()).await;
    let edited = "---\nname: one\nsteps:\n  - id: a\n    agent: w\n    body: |\n      a changed\n---\nprelude\n";
    let fake = FakeExecutor::new();
    match resume_workflow(&plan(edited), &no_args(), &fake, &mut journal).await {
        Err(WorkflowError::Divergence { detail }) => {
            assert!(detail.contains("plan hash mismatch"), "{detail}")
        }
        other => panic!("expected divergence, got {other:?}"),
    }
}

#[tokio::test]
async fn changed_args_diverge_on_resume() {
    let dir = temp_dir("divargs");
    let text = "---\nname: one\nargs:\n  w: { type: string }\nsteps:\n  - id: a\n    agent: w\n    body: |\n      use {{arg.w}}\n---\nprelude\n";
    let mut provided = std::collections::BTreeMap::new();
    provided.insert(
        "w".to_string(),
        craft_workflow::ArgValue::Str("x".to_string()),
    );
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    run_workflow(&plan(text), &provided, &FakeExecutor::new(), &mut journal)
        .await
        .unwrap();
    let mut other_args = std::collections::BTreeMap::new();
    other_args.insert(
        "w".to_string(),
        craft_workflow::ArgValue::Str("y".to_string()),
    );
    match resume_workflow(&plan(text), &other_args, &FakeExecutor::new(), &mut journal).await {
        Err(WorkflowError::Divergence { detail }) => {
            assert!(detail.contains("args digest"), "{detail}")
        }
        other => panic!("expected divergence, got {other:?}"),
    }
}

#[tokio::test]
async fn edited_dep_preview_diverges_the_downstream_step() {
    let dir = temp_dir("divprev");
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    run_workflow(
        &plan(plan_texts::CHAIN),
        &no_args(),
        &FakeExecutor::new(),
        &mut journal,
    )
    .await
    .unwrap();
    drop(journal);
    let text = String::from_utf8(file_bytes(&dir)).unwrap();
    let tampered = text.replace("\"final_message\":\"did a\"", "\"final_message\":\"did A\"");
    assert_ne!(text, tampered);
    rewrite(&dir, tampered.as_bytes());
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    match resume_workflow(
        &plan(plan_texts::CHAIN),
        &no_args(),
        &FakeExecutor::new(),
        &mut journal,
    )
    .await
    {
        Err(WorkflowError::Divergence { detail }) => {
            assert!(detail.contains("step `b` input hash mismatch"), "{detail}");
        }
        other => panic!("expected divergence, got {other:?}"),
    }
}

#[tokio::test]
async fn tampered_run_finished_diverges() {
    let dir = temp_dir("divterm");
    run_one(&dir, &FakeExecutor::new()).await;
    let text = String::from_utf8(file_bytes(&dir)).unwrap();
    let tampered = text.replace("\"failed_step\":null", "\"failed_step\":\"a\"");
    assert_ne!(text, tampered);
    rewrite(&dir, tampered.as_bytes());
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    match resume_workflow(
        &plan(ONE_STEP),
        &no_args(),
        &FakeExecutor::new(),
        &mut journal,
    )
    .await
    {
        Err(WorkflowError::Divergence { detail }) => {
            assert!(detail.contains("failed_step"), "{detail}")
        }
        other => panic!("expected divergence, got {other:?}"),
    }
}

#[tokio::test]
async fn duplicate_settlement_is_rejected() {
    let dir = temp_dir("dupsettle");
    run_one(&dir, &FakeExecutor::new()).await;
    let text = String::from_utf8(file_bytes(&dir)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut rebuilt = lines[..3].join("\n");
    rebuilt.push('\n');
    let duplicate = lines[2].replace("\"seq\":2,", "\"seq\":3,");
    rebuilt.push_str(&duplicate);
    rebuilt.push('\n');
    rewrite(&dir, rebuilt.as_bytes());
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    match resume_workflow(
        &plan(ONE_STEP),
        &no_args(),
        &FakeExecutor::new(),
        &mut journal,
    )
    .await
    {
        Err(WorkflowError::JournalParse { detail, .. }) => {
            assert!(detail.contains("settles twice"), "{detail}")
        }
        other => panic!("expected a corruption error, got {other:?}"),
    }
}

#[tokio::test]
async fn skip_over_a_healthy_dependency_is_rejected() {
    let dir = temp_dir("badskip");
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    run_workflow(
        &plan(plan_texts::CHAIN),
        &no_args(),
        &FakeExecutor::new(),
        &mut journal,
    )
    .await
    .unwrap();
    drop(journal);
    let text = String::from_utf8(file_bytes(&dir)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut rebuilt = lines[..4].join("\n");
    rebuilt.push('\n');
    rebuilt.push_str("{\"type\":\"step_skipped\",\"seq\":4,\"id\":\"c\",\"reason\":{\"kind\":\"dependency_failed\",\"dep\":\"a\"}}\n");
    rewrite(&dir, rebuilt.as_bytes());
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    match resume_workflow(
        &plan(plan_texts::CHAIN),
        &no_args(),
        &FakeExecutor::new(),
        &mut journal,
    )
    .await
    {
        Err(WorkflowError::JournalParse { detail, .. }) => {
            assert!(
                detail.contains("skipped over healthy dependency"),
                "{detail}"
            );
        }
        other => panic!("expected a corruption error, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_step_in_the_journal_is_rejected() {
    let dir = temp_dir("ghoststep");
    run_one(&dir, &FakeExecutor::new()).await;
    let text = String::from_utf8(file_bytes(&dir)).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut rebuilt = lines[..2].join("\n");
    rebuilt.push('\n');
    rebuilt.push_str("{\"type\":\"step_started\",\"seq\":2,\"id\":\"ghost\"}\n");
    rewrite(&dir, rebuilt.as_bytes());
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    match resume_workflow(
        &plan(ONE_STEP),
        &no_args(),
        &FakeExecutor::new(),
        &mut journal,
    )
    .await
    {
        Err(WorkflowError::JournalParse { detail, .. }) => {
            assert!(detail.contains("ghost"), "{detail}")
        }
        other => panic!("expected a corruption error, got {other:?}"),
    }
}
