//! Pool, retry, and failure-termination corpus over the fake executor.
#![allow(clippy::unwrap_used)]
mod common;

use common::{FakeExecutor, journal_path, no_args, plan_texts, temp_dir, usage};
use craft_workflow::{
    Journal, JournalEvent, StepConclusion, WorkflowError, WorkflowPlan, resume_workflow,
    run_workflow,
};

fn plan(text: &str) -> WorkflowPlan {
    WorkflowPlan::parse("t.md", text).unwrap()
}

fn journal_open(dir: &std::path::Path) -> Journal {
    Journal::open(&journal_path(dir)).unwrap()
}

#[tokio::test]
async fn retry_re_runs_a_failed_step_and_journals_one_settle() {
    let text = r#"---
name: retry-chain
steps:
  - id: a
    agent: w
    body: |
      a
  - id: b
    agent: w
    needs: [a]
    retry: 1
    body: |
      b
  - id: c
    agent: w
    needs: [b]
    body: |
      c
---
prelude
"#;
    let dir = temp_dir("retry");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    fake.queue_fail("b", StepConclusion::Error);
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(fake.calls(), ["a", "b", "b", "c"]);
    assert!(report.is_success());
    let settled: Vec<_> = journal
        .entries()
        .iter()
        .filter(|e| matches!(e, JournalEvent::StepFinished { id, .. } if id == "b"))
        .collect();
    assert_eq!(settled.len(), 1);
    let started: Vec<_> = journal
        .entries()
        .iter()
        .filter(|e| matches!(e, JournalEvent::StepStarted { id, .. } if id == "b"))
        .collect();
    assert_eq!(started.len(), 2);
    match settled[0] {
        JournalEvent::StepFinished {
            usage: settled_usage,
            conclusion,
            ..
        } => {
            assert_eq!(*conclusion, StepConclusion::Success);
            assert_eq!(*settled_usage, usage(4, 120, 60, 20));
        }
        _ => panic!(),
    }
    assert_eq!(report.total_usage, usage(8, 240, 120, 40));
}

#[tokio::test]
async fn retry_ceiling_is_clamped_by_limits() {
    let text = r#"---
name: clamp
limits:
  max-retries-per-step: 1
steps:
  - id: a
    agent: w
    retry: 4
    body: |
      a
---
prelude
"#;
    let dir = temp_dir("clamp");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    fake.queue_fail("a", StepConclusion::Error);
    fake.queue_fail("a", StepConclusion::Error);
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(fake.calls(), ["a", "a"]);
    assert_eq!(report.exit_code, 1);
}

#[tokio::test]
async fn pool_refused_retry_settles_the_step_with_its_failure() {
    let text = r#"---
name: refused
limits:
  max-total-tokens: 50
steps:
  - id: a
    agent: w
    retry: 1
    body: |
      a
---
prelude
"#;
    let dir = temp_dir("refused");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    fake.queue_fail("a", StepConclusion::Error);
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(fake.calls(), ["a"]);
    assert_eq!(report.exit_code, 1);
    assert_eq!(report.failed_step.as_deref(), Some("a"));
    let settled: Vec<_> = journal
        .entries()
        .iter()
        .filter(|e| matches!(e, JournalEvent::StepFinished { id, .. } if id == "a"))
        .collect();
    assert_eq!(settled.len(), 1);
}

#[tokio::test]
async fn executor_launch_failure_is_a_typed_error_and_stays_resumable() {
    let dir = temp_dir("launch");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    fake.launch_error("c", "provider unreachable");
    let result = run_workflow(&plan(plan_texts::CHAIN), &no_args(), &fake, &mut journal).await;
    match result.unwrap_err() {
        WorkflowError::StepLaunch { step, detail } => {
            assert_eq!(step, "c");
            assert!(detail.contains("provider unreachable"));
        }
        other => panic!("expected a step launch error, got {other:?}"),
    }
    assert!(matches!(
        journal.entries().first(),
        Some(JournalEvent::RunStarted { .. })
    ));
    assert!(
        matches!(journal.entries().last(), Some(JournalEvent::StepStarted { id, .. }) if id == "c")
    );
    assert!(
        !journal
            .entries()
            .iter()
            .any(|e| matches!(e, JournalEvent::RunFinished { .. }))
    );
}

#[tokio::test]
async fn args_render_into_prompts_with_defaults() {
    let dir = temp_dir("args");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    let report = run_workflow(
        &plan(plan_texts::ARGS_PLAN),
        &no_args(),
        &fake,
        &mut journal,
    )
    .await
    .unwrap();
    assert!(report.is_success());
    let prompt = fake.prompts_for("only");
    assert!(
        prompt[0].contains("scan since 24h over alpha, beta"),
        "prompt: {}",
        prompt[0]
    );
    assert!(prompt[0].contains("prelude text"));
}

#[tokio::test]
async fn running_over_a_dirty_journal_is_refused() {
    let dir = temp_dir("dirty");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    run_workflow(&plan(plan_texts::CHAIN), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    let result = run_workflow(&plan(plan_texts::CHAIN), &no_args(), &fake, &mut journal).await;
    assert!(matches!(
        result.unwrap_err(),
        WorkflowError::JournalNotEmpty { entries: 8, .. }
    ));
}

#[tokio::test]
async fn resuming_an_empty_journal_is_refused() {
    let dir = temp_dir("emptyj");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    let result = resume_workflow(&plan(plan_texts::CHAIN), &no_args(), &fake, &mut journal).await;
    assert!(matches!(
        result.unwrap_err(),
        WorkflowError::JournalEmpty { .. }
    ));
}
