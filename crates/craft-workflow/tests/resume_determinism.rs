//! Resume and determinism: fold-equals-live, run-only-what-missed, prior
//! usage counting, raised-limit resume, and byte-identical replays.
#![allow(clippy::unwrap_used)]

mod common;

use common::{FakeExecutor, journal_path, no_args, plan_texts, temp_dir, usage};
use craft_workflow::{
    Journal, JournalEvent, RunConclusion, SkipReason, StepStatus, WorkflowPlan, resume_workflow,
    run_workflow,
};

fn plan(text: &str) -> WorkflowPlan {
    WorkflowPlan::parse("t.md", text).unwrap()
}

fn open(dir: &std::path::Path) -> Journal {
    Journal::open(&journal_path(dir)).unwrap()
}

#[tokio::test]
async fn resume_of_a_finished_journal_replays_without_execution() {
    let dir = temp_dir("replay");
    let mut journal = open(&dir);
    let fake = FakeExecutor::new();
    let live = run_workflow(&plan(plan_texts::CHAIN), &no_args(), &fake, &mut journal)
        .await
        .unwrap();
    drop(journal);
    let fresh = FakeExecutor::new();
    let mut journal = open(&dir);
    let replayed = resume_workflow(&plan(plan_texts::CHAIN), &no_args(), &fresh, &mut journal)
        .await
        .unwrap();
    assert_eq!(fresh.calls().len(), 0);
    assert_eq!(live, replayed);
    assert_eq!(journal.len(), 8);
}

#[tokio::test]
async fn resume_after_an_interruption_runs_only_the_missing_step() {
    let dir = temp_dir("interrupt");
    let mut journal = open(&dir);
    let first = FakeExecutor::new();
    first.launch_error("c", "boom");
    let result = run_workflow(&plan(plan_texts::CHAIN), &no_args(), &first, &mut journal).await;
    assert!(result.is_err());
    drop(first);
    drop(journal);
    let mut journal = open(&dir);
    let second = FakeExecutor::new();
    let report = resume_workflow(&plan(plan_texts::CHAIN), &no_args(), &second, &mut journal)
        .await
        .unwrap();
    assert_eq!(second.calls(), ["c"]);
    assert!(report.is_success());
    assert_eq!(report.total_usage, usage(6, 180, 90, 30));
    let finished_a: Vec<_> = journal
        .entries()
        .iter()
        .filter(|e| matches!(e, JournalEvent::StepFinished { id, .. } if id == "a"))
        .collect();
    assert_eq!(finished_a.len(), 1);
    assert!(matches!(
        journal.entries().last(),
        Some(JournalEvent::RunFinished { .. })
    ));
}

#[tokio::test]
async fn prior_usage_still_counts_against_the_pool_on_resume() {
    let loose = plan_texts::CHAIN.replace(
        "name: chain",
        "name: chain\nlimits:\n  max-total-tokens: 250",
    );
    let tight = plan_texts::CHAIN.replace(
        "name: chain",
        "name: chain\nlimits:\n  max-total-tokens: 199",
    );
    assert_eq!(plan(&loose).plan_hash, plan(&tight).plan_hash);
    let dir = temp_dir("seedlow");
    let mut journal = open(&dir);
    let first = FakeExecutor::new();
    first.launch_error("c", "boom");
    let result = run_workflow(&plan(&loose), &no_args(), &first, &mut journal).await;
    assert!(result.is_err());
    drop(journal);
    let mut journal = open(&dir);
    let second = FakeExecutor::new();
    let report = resume_workflow(&plan(&tight), &no_args(), &second, &mut journal)
        .await
        .unwrap();
    assert_eq!(second.calls().len(), 0);
    assert!(matches!(report.conclusion, RunConclusion::BudgetExhausted));
    assert_eq!(report.exit_code, 3);
    match report.steps.iter().find(|s| s.id == "c").map(|s| &s.status) {
        Some(StepStatus::Skipped { reason }) => {
            assert_eq!(*reason, SkipReason::Unreachable);
        }
        other => panic!("expected a visible skip, got {other:?}"),
    }
}

#[tokio::test]
async fn resume_with_raised_limits_completes_the_missing_step() {
    let tight = plan_texts::CHAIN.replace(
        "name: chain",
        "name: chain\nlimits:\n  max-total-tokens: 250",
    );
    let raised = plan_texts::CHAIN.replace(
        "name: chain",
        "name: chain\nlimits:\n  max-total-tokens: 400",
    );
    assert_eq!(plan(&tight).plan_hash, plan(&raised).plan_hash);
    let dir = temp_dir("seedhigh");
    let mut journal = open(&dir);
    let first = FakeExecutor::new();
    first.launch_error("c", "boom");
    let result = run_workflow(&plan(&tight), &no_args(), &first, &mut journal).await;
    assert!(result.is_err());
    drop(journal);
    let mut journal = open(&dir);
    let second = FakeExecutor::new();
    let report = resume_workflow(&plan(&raised), &no_args(), &second, &mut journal)
        .await
        .unwrap();
    assert_eq!(second.calls(), ["c"]);
    assert!(report.is_success());
    assert_eq!(report.total_usage, usage(6, 180, 90, 30));
}

#[tokio::test]
async fn same_plan_and_executor_produce_byte_identical_journals() {
    let first_dir = temp_dir("det1");
    let second_dir = temp_dir("det2");
    let mut first_journal = open(&first_dir);
    run_workflow(
        &plan(plan_texts::CHAIN),
        &no_args(),
        &FakeExecutor::new(),
        &mut first_journal,
    )
    .await
    .unwrap();
    let mut second_journal = open(&second_dir);
    let second_report = run_workflow(
        &plan(plan_texts::CHAIN),
        &no_args(),
        &FakeExecutor::new(),
        &mut second_journal,
    )
    .await
    .unwrap();
    drop(first_journal);
    drop(second_journal);
    let first_bytes = std::fs::read(journal_path(&first_dir)).unwrap();
    let second_bytes = std::fs::read(journal_path(&second_dir)).unwrap();
    assert_eq!(first_bytes, second_bytes);
    let mut first_journal = open(&first_dir);
    let first_report = resume_workflow(
        &plan(plan_texts::CHAIN),
        &no_args(),
        &FakeExecutor::new(),
        &mut first_journal,
    )
    .await
    .unwrap();
    assert_eq!(first_report, second_report);
}

#[tokio::test]
async fn diamond_replays_identically_at_one_slot() {
    let first_dir = temp_dir("dia1");
    let second_dir = temp_dir("dia2");
    for dir in [&first_dir, &second_dir] {
        let mut journal = open(dir);
        run_workflow(
            &plan(plan_texts::DIAMOND),
            &no_args(),
            &FakeExecutor::new(),
            &mut journal,
        )
        .await
        .unwrap();
    }
    let first_bytes = std::fs::read(journal_path(&first_dir)).unwrap();
    let second_bytes = std::fs::read(journal_path(&second_dir)).unwrap();
    assert_eq!(first_bytes, second_bytes);
}

#[tokio::test]
async fn terminal_settle_is_exactly_once_on_every_path() {
    let dirs = [
        temp_dir("term-ok"),
        temp_dir("term-fail"),
        temp_dir("term-crash"),
    ];
    for (i, dir) in dirs.iter().enumerate() {
        let mut journal = open(dir);
        let fake = FakeExecutor::new();
        if i == 1 {
            fake.queue_fail("b", craft_workflow::StepConclusion::Budget);
        }
        if i == 2 {
            fake.launch_error("c", "boom");
        }
        let _ = run_workflow(&plan(plan_texts::CHAIN), &no_args(), &fake, &mut journal).await;
        let finished: Vec<_> = journal
            .entries()
            .iter()
            .filter(|e| matches!(e, JournalEvent::RunFinished { .. }))
            .collect();
        assert_eq!(finished.len(), if i == 2 { 0 } else { 1 }, "dir {dir:?}");
    }
}
