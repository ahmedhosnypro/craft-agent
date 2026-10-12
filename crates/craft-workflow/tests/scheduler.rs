//! Scheduler corpus: ordering, slot caps, failure semantics, retries, and
//! the fail-closed pool, all over the fake executor.
#![allow(clippy::unwrap_used)]
mod common;

use common::{FakeExecutor, journal_path, no_args, plan_texts, temp_dir, usage};
use craft_workflow::{
    Journal, JournalEvent, RunConclusion, StepConclusion, StepStatus, WorkflowPlan, run_workflow,
};

fn plan(text: &str) -> WorkflowPlan {
    WorkflowPlan::parse("t.md", text).unwrap()
}

fn report_status(report: &craft_workflow::WorkflowReport, id: &str) -> StepStatus {
    report
        .steps
        .iter()
        .find(|s| s.id == id)
        .unwrap()
        .status
        .clone()
}

async fn run(text: &str) -> (craft_workflow::WorkflowReport, FakeExecutor) {
    let dir = temp_dir("sched");
    let mut journal = Journal::open(&journal_path(&dir)).unwrap();
    let fake = FakeExecutor::new();
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    (report, fake)
}

fn journal_open(dir: &std::path::Path) -> Journal {
    Journal::open(&journal_path(dir)).unwrap()
}

#[tokio::test]
async fn linear_chain_runs_in_order_and_journals_each_settle() {
    let (report, fake) = run(plan_texts::CHAIN).await;
    assert_eq!(fake.calls(), ["a", "b", "c"]);
    assert!(report.is_success());
    assert_eq!(report.exit_code, 0);
    assert_eq!(report.total_usage, usage(6, 180, 90, 30));
    assert!(matches!(
        report_status(&report, "a"),
        StepStatus::Settled {
            conclusion: StepConclusion::Success,
            ..
        }
    ));
}

#[tokio::test]
async fn downstream_prompts_carry_the_prelude_and_dep_previews() {
    let (report, fake) = run(plan_texts::CHAIN).await;
    let prompt_b = fake.prompts_for("b");
    assert_eq!(prompt_b.len(), 1);
    assert!(
        prompt_b[0].contains("# Shared rules"),
        "prelude missing: {}",
        prompt_b[0]
    );
    assert!(prompt_b[0].contains("## Prior steps"));
    assert!(prompt_b[0].contains("### a"));
    assert!(prompt_b[0].contains("did a"));
    assert!(prompt_b[0].contains("do b"));
    let prompt_a = fake.prompts_for("a");
    assert!(prompt_a[0].contains("do a"));
    assert!(!prompt_a[0].contains("## Prior steps"));
    assert_eq!(report.exit_code, 0);
}

#[tokio::test]
async fn diamond_admits_in_declaration_order_at_default_slots() {
    let (report, fake) = run(plan_texts::DIAMOND).await;
    assert_eq!(fake.calls(), ["top", "left", "right", "join"]);
    assert_eq!(fake.max_in_flight(), 1);
    assert!(report.is_success());
}

#[tokio::test]
async fn slot_cap_bounds_in_flight_steps() {
    let text = plan_texts::FOUR_INDEPENDENT
        .replace("max-steps: 4", "max-steps: 4\n  max-concurrent-steps: 2");
    let (report, fake) = run(&text).await;
    assert_eq!(fake.max_in_flight(), 2);
    assert!(report.is_success());
    assert_eq!(fake.calls().len(), 4);
}

#[tokio::test]
async fn stop_on_failure_skips_dependants_visibly() {
    let dir = temp_dir("stop");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    fake.queue_fail("b", StepConclusion::MaxTurns);
    let report = run_workflow(&plan(plan_texts::CHAIN), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(report.exit_code, 2);
    assert_eq!(report.failed_step.as_deref(), Some("b"));
    match &report.conclusion {
        RunConclusion::Failed { step, conclusion } => {
            assert_eq!(step, "b");
            assert_eq!(*conclusion, StepConclusion::MaxTurns);
        }
        other => panic!("expected failure, got {other:?}"),
    }
    match report_status(&report, "c") {
        StepStatus::Skipped { reason } => {
            assert_eq!(
                reason,
                craft_workflow::SkipReason::DependencyFailed {
                    dep: "b".to_string()
                }
            );
        }
        other => panic!("expected visible skip, got {other:?}"),
    }
    let skipped_rows: Vec<_> = journal
        .entries()
        .iter()
        .filter(|e| matches!(e, JournalEvent::StepSkipped { id, .. } if id == "c"))
        .collect();
    assert_eq!(skipped_rows.len(), 1);
    assert!(matches!(
        journal.entries().last(),
        Some(JournalEvent::RunFinished { .. })
    ));
}

#[tokio::test]
async fn first_failing_step_carries_the_exit_code() {
    let text = r#"---
name: two
steps:
  - id: a
    agent: w
    on-failure: skip
    body: |
      a
  - id: b
    agent: w
    body: |
      b
---
prelude
"#;
    let dir = temp_dir("first");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    fake.queue_fail("a", StepConclusion::Error);
    fake.queue_fail("b", StepConclusion::Denied);
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(report.exit_code, 1);
    assert_eq!(report.failed_step.as_deref(), Some("a"));
    assert_eq!(fake.calls(), ["a", "b"]);
}

#[tokio::test]
async fn skip_leaf_failure_lets_the_rest_of_the_run_continue() {
    let text = r#"---
name: leaf
steps:
  - id: best_effort
    agent: w
    on-failure: skip
    body: |
      optional
  - id: core
    agent: w
    body: |
      required
---
prelude
"#;
    let dir = temp_dir("leaf");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    fake.queue_fail("best_effort", StepConclusion::Stationarity);
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(report.exit_code, 2);
    assert_eq!(report.failed_step.as_deref(), Some("best_effort"));
    match report_status(&report, "core") {
        StepStatus::Settled { conclusion, .. } => assert_eq!(conclusion, StepConclusion::Success),
        other => panic!("core should still run, got {other:?}"),
    }
}
