//! Pool admission corpus: the fail-closed step/turn/token checks that
//! refuse a step before it starts.
#![allow(clippy::unwrap_used)]

mod common;

use common::{FakeExecutor, journal_path, no_args, plan_texts, temp_dir};
use craft_workflow::{RunConclusion, StepStatus, WorkflowPlan, run_workflow};

fn plan(text: &str) -> WorkflowPlan {
    WorkflowPlan::parse("t.md", text).unwrap()
}

fn journal_open(dir: &std::path::Path) -> craft_workflow::Journal {
    craft_workflow::Journal::open(&journal_path(dir)).unwrap()
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

#[tokio::test]
async fn step_cap_refuses_admission_and_exhausts_budget() {
    let text = plan_texts::FOUR_INDEPENDENT.replace("max-steps: 4", "max-steps: 2");
    let dir = temp_dir("stepcap");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    let report = run_workflow(&plan(&text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(report.exit_code, 3);
    assert_eq!(report.failed_step, None);
    assert!(matches!(report.conclusion, RunConclusion::BudgetExhausted));
    assert_eq!(fake.calls(), ["w", "x"]);
    match report_status(&report, "z") {
        StepStatus::Skipped { reason } => {
            assert_eq!(reason, craft_workflow::SkipReason::Unreachable)
        }
        other => panic!("expected a visible skip, got {other:?}"),
    }
}

#[tokio::test]
async fn turn_reservation_refuses_admission_before_the_next_start() {
    let text = r#"---
name: turns
limits:
  max-total-turns: 3
steps:
  - id: a
    agent: w
    max-turns: 2
    body: |
      a
  - id: b
    agent: w
    max-turns: 2
    body: |
      b
---
prelude
"#;
    let dir = temp_dir("turns");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(fake.calls(), ["a"]);
    assert!(matches!(report.conclusion, RunConclusion::BudgetExhausted));
    assert_eq!(report.exit_code, 3);
    match report_status(&report, "b") {
        StepStatus::Skipped { reason } => {
            assert_eq!(reason, craft_workflow::SkipReason::Unreachable)
        }
        other => panic!("expected a visible skip, got {other:?}"),
    }
}

#[tokio::test]
async fn token_pool_exhaustion_refuses_later_admissions() {
    let text = r#"---
name: tokens
limits:
  max-total-tokens: 50
steps:
  - id: a
    agent: w
    body: |
      a
  - id: b
    agent: w
    body: |
      b
---
prelude
"#;
    let dir = temp_dir("tokens");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(fake.calls(), ["a"]);
    assert!(matches!(report.conclusion, RunConclusion::BudgetExhausted));
}

#[tokio::test]
async fn completion_before_budget_lets_the_last_step_finish() {
    let text = r#"---
name: completes
limits:
  max-total-tokens: 150
steps:
  - id: a
    agent: w
    body: |
      a
  - id: b
    agent: w
    body: |
      b
---
prelude
"#;
    let dir = temp_dir("complete");
    let mut journal = journal_open(&dir);
    let fake = FakeExecutor::new();
    let report = run_workflow(&plan(text), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    assert_eq!(fake.calls(), ["a", "b"]);
    assert!(report.is_success());
    assert!(report.total_usage.total_tokens() > 150);
}
