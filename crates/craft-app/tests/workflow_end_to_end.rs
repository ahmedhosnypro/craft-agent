//! RED corpus: the workflow engine wired end to end — plan file to
//! journal to per-step executor runs to the terminal report, plus the
//! two stream-contract events and exit mapping.
#![allow(clippy::expect_used)]

#[path = "workflow_end_to_end/stream_contract.rs"]
mod stream_contract;
mod workflow_common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use craft_app::stream_json::{write_event, write_plain_event};
use craft_app::workflow_cmd::{run_or_resume, workflow_run_dir};
use craft_app::workflow_exec::{StepMount, WorkflowExecutor};
use craft_core::error::ProviderError;
use craft_runtime::RuntimeEvent;
use craft_workflow::WorkflowReport;
use craft_workflow::plan::ArgValue;
use craft_workflow::run::{args_digest, run_id_for};
use craft_workflow::{Journal, WorkflowPlan, plan};

use workflow_common::{FakeProvider, Reply, repo, text_reply, usage, write_step_agent};

const TWO_STEP_PLAN: &str = r#"---
name: two-step
steps:
  - id: first
    agent: step-agent
    max-turns: 3
    retry: 0
    on-failure: stop
    outputs: [a]
    body: |
      First step body.
  - id: second
    agent: step-agent
    needs: [first]
    max-turns: 3
    retry: 0
    on-failure: stop
    outputs: [b]
    body: |
      Second step body.
---

Shared rules.
"#;

const STARVED_PLAN: &str = r#"---
name: starved
limits:
  max-total-turns: 1
steps:
  - id: only
    agent: step-agent
    max-turns: 3
    retry: 0
    on-failure: stop
    body: |
      Never admitted.
---
"#;

struct Rig {
    workspace: std::path::PathBuf,
    plan: WorkflowPlan,
    run_dir: std::path::PathBuf,
    args: BTreeMap<String, ArgValue>,
    run_id: String,
    digest: String,
}

fn rig(name: &str, body: &str) -> Result<Rig, Box<dyn std::error::Error>> {
    let workspace = repo(name)?;
    write_step_agent(&workspace)?;
    let path = workspace.join("plan.md");
    fs::write(&path, body)?;
    let plan = WorkflowPlan::parse_file(&path)?;
    let args = plan::resolve_args(&plan, &BTreeMap::new())?;
    let digest = args_digest(&args)?;
    let run_id = run_id_for(&plan.plan_hash, &digest);
    let run_dir = workflow_run_dir(&workspace, &run_id);
    fs::create_dir_all(run_dir.join("sessions"))?;
    Ok(Rig {
        workspace,
        plan,
        run_dir,
        args,
        run_id,
        digest,
    })
}

fn executor(
    rig: &Rig,
    provider: std::sync::Arc<FakeProvider>,
) -> Result<WorkflowExecutor<FakeProvider>, craft_app::AppError> {
    let agents = craft_app::workflow_cmd::resolve_step_agents(&rig.plan, &rig.workspace)?;
    let mount: StepMount<FakeProvider> = std::sync::Arc::new(move |_model| Ok(provider.clone()));
    Ok(WorkflowExecutor::new(
        mount,
        "base-model".to_string(),
        rig.workspace.clone(),
        rig.run_dir.join("sessions"),
        Some(rig.plan.limits.max_total_tokens),
        None,
        Some(3),
        agents,
        craft_runtime::ContextKnobs::default(),
    ))
}

fn journal(rig: &Rig) -> Result<Journal, craft_workflow::WorkflowError> {
    Journal::open(&rig.run_dir.join("journal.jsonl"))
}

fn two_step_provider() -> std::sync::Arc<FakeProvider> {
    FakeProvider::new(vec![
        text_reply("first done", usage(10, 2)),
        text_reply("second done", usage(20, 4)),
    ])
}

fn assert_two_step_success(report: &WorkflowReport) {
    assert_eq!(report.exit_code, 0);
    assert!(report.is_success());
    assert_eq!(report.steps.len(), 2);
    let usage = &report.total_usage;
    assert_eq!(usage.turns, 2);
    assert_eq!(usage.input_tokens, 30);
}

fn assert_prompt_carries_the_preview(request: &craft_core::types::ModelRequest) {
    let prompt: String = request
        .messages
        .iter()
        .map(|message| match message.as_ref() {
            craft_core::types::ChatMessage::User { text, .. } => text.as_str(),
            _ => "",
        })
        .collect();
    assert!(
        prompt.contains("## Prior steps"),
        "the dependant step sees the upstream previews"
    );
    assert!(
        prompt.contains("first done"),
        "the settled preview rides the dependant prompt"
    );
}

fn assert_replayed_report(replay: &WorkflowReport, first: &WorkflowReport) {
    assert_eq!(replay.exit_code, 0);
    assert_eq!(replay.steps, first.steps);
}

fn workflow_events(rig: &Rig, report: &WorkflowReport) -> (RuntimeEvent, RuntimeEvent) {
    let started = RuntimeEvent::WorkflowStarted {
        run_id: rig.run_id.clone(),
        name: rig.plan.name.clone(),
        plan_hash: rig.plan.plan_hash.clone(),
        args_digest: rig.digest.clone(),
    };
    let finished = RuntimeEvent::WorkflowFinished {
        exit_code: report.exit_code,
        report: serde_json::to_value(report).expect("report serializes"),
    };
    (started, finished)
}

fn event_json(event: &RuntimeEvent) -> serde_json::Value {
    let mut out = Vec::new();
    write_event(&mut out, event).expect("event line writes");
    let text = String::from_utf8(out).expect("valid utf-8");
    serde_json::from_str(text.lines().next().expect("one event per line")).expect("event json")
}

fn assert_started_event_json(rig: &Rig, started: &RuntimeEvent) {
    let value = event_json(started);
    assert_eq!(value["type"], "workflow_started");
    assert_eq!(value["name"], "two-step");
    assert_started_identity(rig, &value);
}

fn assert_started_identity(rig: &Rig, value: &serde_json::Value) {
    assert_eq!(value["run_id"], rig.run_id);
    assert_eq!(value["plan_hash"], rig.plan.plan_hash);
    assert_eq!(value["args_digest"], rig.digest);
}

fn assert_finished_event_json(finished: &RuntimeEvent) {
    let value = event_json(finished);
    assert_eq!(value["type"], "workflow_finished");
    assert_eq!(value["exit_code"], 0);
    assert_finished_report(&value["report"]);
}

fn assert_finished_report(report: &serde_json::Value) {
    assert_eq!(report["conclusion"]["kind"], "completed");
    assert_eq!(report["steps"].as_array().map(Vec::len), Some(2));
}

fn assert_workflow_plain_lines(started: &RuntimeEvent, finished: &RuntimeEvent) {
    let mut plain = Vec::new();
    write_plain_event(&mut plain, started).expect("plain started writes");
    write_plain_event(&mut plain, finished).expect("plain finished writes");
    let plain = String::from_utf8(plain).expect("valid utf-8");
    assert!(plain.contains("workflow `two-step` started"), "{plain}");
    assert!(plain.contains("workflow finished: exit 0"), "{plain}");
}

#[tokio::test]
async fn a_two_step_workflow_completes_and_journals() {
    let rig = rig("e2e-two-step", TWO_STEP_PLAN).expect("rig");
    let provider = two_step_provider();
    let executor = executor(&rig, provider.clone()).expect("executor");
    let mut log = journal(&rig).expect("journal");
    let report = run_or_resume(&rig.plan, &rig.args, &executor, &mut log)
        .await
        .expect("workflow runs");
    assert_two_step_success(&report);
    assert_eq!(log.len(), 6, "run_started, two step pairs, run_finished");
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    assert_prompt_carries_the_preview(&requests[1]);
}

#[tokio::test]
async fn a_failed_step_stops_dependants_and_maps_the_exit_code() {
    let rig = rig("e2e-failure", TWO_STEP_PLAN).expect("rig");
    let provider = FakeProvider::new(vec![Reply::Fail(ProviderError::Auth {
        detail: "bad key".to_string(),
    })]);
    let executor = executor(&rig, provider.clone()).expect("executor");
    let mut log = journal(&rig).expect("journal");
    let report = run_or_resume(&rig.plan, &rig.args, &executor, &mut log)
        .await
        .expect("workflow runs");
    assert_eq!(report.exit_code, 1);
    assert!(!report.is_success());
    assert_eq!(report.failed_step.as_deref(), Some("first"));
    let second = &report.steps[1];
    assert!(
        matches!(
            &second.status,
            craft_workflow::StepStatus::Skipped {
                reason: craft_workflow::SkipReason::DependencyFailed { dep }
            } if dep == "first"
        ),
        "the dependant skips visibly: {:?}",
        second.status
    );
}

#[tokio::test]
async fn a_finished_journal_resumes_as_pure_replay() {
    let rig = rig("e2e-resume", TWO_STEP_PLAN).expect("rig");
    let provider = two_step_provider();
    let step_runner = executor(&rig, provider).expect("executor");
    let mut log = journal(&rig).expect("journal");
    let first = run_or_resume(&rig.plan, &rig.args, &step_runner, &mut log)
        .await
        .expect("workflow runs");
    assert_eq!(first.exit_code, 0);

    let replay_provider = FakeProvider::new(Vec::new());
    let replay_runner = executor(&rig, replay_provider.clone()).expect("executor");
    let mut replay_log = journal(&rig).expect("journal");
    let replay = run_or_resume(&rig.plan, &rig.args, &replay_runner, &mut replay_log)
        .await
        .expect("replay runs");
    assert_replayed_report(&replay, &first);
    assert_eq!(
        replay_provider.calls(),
        0,
        "a finished journal replays without new provider calls"
    );
    assert_eq!(replay_log.len(), 6, "replay appends nothing");
}

#[tokio::test]
async fn budget_refusal_ends_the_workflow_as_exhausted() {
    let rig = rig("e2e-budget", STARVED_PLAN).expect("rig");
    let provider = FakeProvider::new(vec![text_reply("never", usage(1, 1))]);
    let executor = executor(&rig, provider.clone()).expect("executor");
    let mut log = journal(&rig).expect("journal");
    let report = run_or_resume(&rig.plan, &rig.args, &executor, &mut log)
        .await
        .expect("workflow runs");
    assert_eq!(report.exit_code, 3);
    assert!(
        matches!(
            report.conclusion,
            craft_workflow::RunConclusion::BudgetExhausted
        ),
        "the pool refusal is terminal: {:?}",
        report.conclusion
    );
    assert_eq!(provider.calls(), 0, "a refused step never runs");
}

#[test]
fn the_run_dir_reuses_the_deterministic_run_id() {
    let first = rig("e2e-runid", TWO_STEP_PLAN).expect("rig");
    let second = rig("e2e-runid", TWO_STEP_PLAN).expect("rig");
    assert_eq!(
        first.run_id, second.run_id,
        "same plan and args map to one dir"
    );
    assert!(
        Path::new(&first.run_dir).ends_with(format!(".craft-agent/workflows/{}", first.run_id))
    );
    let other = rig("e2e-rerun", STARVED_PLAN).expect("rig");
    assert_ne!(
        first.run_id, other.run_id,
        "a different plan gets a new dir"
    );
}
