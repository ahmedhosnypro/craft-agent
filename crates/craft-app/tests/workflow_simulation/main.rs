//! The judged-workflow simulation: the same 8-step per-PR-life
//! architecture as the real adoption scan — a scripted parallel sweep,
//! one detached pipeline per PR running that PR's whole life under a
//! two-slot concurrency cap, each pipeline spawning its own stage
//! children while its siblings are still early, panel gates with a
//! repaired seat, an appeal, and one seal — driven end to end over a
//! scripted provider with local fixtures and no network. This runs in
//! seconds and asserts the same proofs the sim's assert-workflow.sh
//! checks on a real run: spawn tools live, nested parallelism reached,
//! every child settled, gates settled, and the run concluded.
#![allow(clippy::expect_used)]

mod rig;
mod scripted;

use std::sync::Arc;

use craft_app::workflow_cmd::run_or_resume;
use craft_app::workflow_exec::{StepMount, WorkflowExecutor};
use craft_workflow::plan;
use craft_workflow::{Journal, WorkflowPlan};

use rig::{Rig, rig as rig_for};
use scripted::SimProvider;

/// The full 8-step simulation over the driver, returning the report.
fn run_simulation(rig: &Rig) -> Result<craft_workflow::WorkflowReport, Box<dyn std::error::Error>> {
    let plan_spec = WorkflowPlan::parse_file(
        &rig.workspace
            .join(".craft-agent")
            .join("workflows")
            .join("sim-adopt.md"),
    )?;
    let args = plan::resolve_args(&plan_spec, &std::collections::BTreeMap::new())?;
    let mount: StepMount<SimProvider> = {
        let provider = rig.provider.clone();
        Arc::new(move |_model| Ok(provider.clone()))
    };
    let mut executor = WorkflowExecutor::new(
        mount,
        "sim-model".to_string(),
        rig.workspace.clone(),
        rig.run_dir.join("sessions"),
        Some(1_000_000),
        None,
        Some(3),
        rig.agents.clone(),
        craft_runtime::ContextKnobs::default(),
    );
    executor = executor
        .with_subagents(Some(rig.hub.clone()))
        .with_sink(rig.sink.clone());
    let mut journal = Journal::open(&rig.run_dir.join("journal.jsonl"))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let report = runtime.block_on(run_or_resume(&plan_spec, &args, &executor, &mut journal))?;
    Ok(report)
}

#[test]
fn the_judged_workflow_simulation_settles_every_step() {
    let rig = rig_for("full");
    let report = run_simulation(&rig).expect("the simulated workflow settles");
    assert_eq!(report.exit_code, 0, "the simulation concludes success");
    assert_eq!(report.steps.len(), 8, "all eight steps settle");
    for step in &report.steps {
        match &step.status {
            craft_workflow::status::StepStatus::Settled { conclusion, .. } => {
                assert!(conclusion.is_success(), "step {} succeeds", step.id)
            }
            skipped => panic!("step {} skips: {skipped:?}", step.id),
        }
    }
}

#[test]
fn the_simulation_proves_spawn_tools_live_and_parallel() {
    let rig = rig_for("spawns");
    let report = run_simulation(&rig).expect("the simulated workflow settles");
    // Every step and every child made model calls through the provider.
    assert!(
        rig.provider.calls() > 20,
        "every step, pipeline, and stage child made model calls: {}",
        rig.provider.calls()
    );
    // The run's event stream carries one launch and one settle per child.
    let (started, finished, peak) = spawn_counts(&rig.events);
    assert_eq!(
        started, 17,
        "the fan-out launches every per-PR pipeline plus each pipeline's\
         classifier, its stage-2 drafter and committer, and the seal's\
         rollup child = {started}"
    );
    assert_eq!(started, finished, "no child is left dangling");
    assert!(
        peak >= 3,
        "children overlap: the peak concurrent fan-out was {peak}"
    );
    // A judge seat answered in prose, forcing the engine's repair loop.
    assert!(
        rig.provider.malformed() >= 1,
        "a judge seat answered in prose, forcing the repair loop"
    );
    // The local artifacts the pipeline produces exist: one verdict file
    // per PR, the sweep markers, and the rollup drafts.
    assert!(
        rig.workspace
            .join("run-ledger/prepare/.collect-done")
            .is_file(),
        "the scripted sweep recorded its collection marker"
    );
    for number in ["app-a#101", "app-b#201", "app-c#301"] {
        assert!(
            rig.workspace
                .join(format!("run-ledger/classifier/{number}.verdict"))
                .is_file(),
            "PR {number} has a verdict file"
        );
    }
    assert!(
        rig.workspace
            .join("run-ledger/rollup/state-body.md")
            .is_file()
    );
    assert_eq!(report.exit_code, 0);
}

#[test]
fn a_seat_repair_loop_lands_the_verdict() {
    // The simulation's seat 1 answers in prose; the engine must repair it
    // until the verdict parses and the gate still settles.
    let rig = rig_for("repair");
    let report = run_simulation(&rig).expect("the repaired gate settles");
    assert_eq!(report.exit_code, 0);
    assert!(rig.provider.malformed() >= 1);
    assert!(
        report.failed_step.is_none(),
        "no step fails despite the malformed seat"
    );
}

#[test]
fn a_pr_advances_while_its_siblings_are_still_early() {
    // The per-PR property this architecture exists for: each PR's whole
    // life — classification, filing, its own rollup section — is its
    // own pipeline's, so an actionable PR reaches its committer while a
    // sibling is still fetching files. The two-slot concurrency cap
    // forces saturation, so the queue and continuation priority are
    // exercised, not assumed.
    let rig = rig_for("per-pr");
    let report = run_simulation(&rig).expect("the simulated workflow settles");
    assert_eq!(report.exit_code, 0, "{report:?}");

    // Six verdicts: one per PR, never a batch report standing in for six.
    for number in [
        "app-a#101",
        "app-b#201",
        "app-c#301",
        "app-a#102",
        "app-b#202",
        "app-d#401",
    ] {
        assert!(
            rig.workspace
                .join(format!("run-ledger/classifier/{number}.verdict"))
                .is_file(),
            "PR {number} has its own verdict file"
        );
    }
    // Two stage-2 packages, one per actionable PR: the rev-sync PR's
    // applier package and the hoist PR's filer package.
    for number in ["app-a#101", "app-b#201"] {
        assert!(
            rig.workspace
                .join(format!("run-ledger/drafts/{number}.md"))
                .is_file(),
            "PR {number} advanced to its stage-2 package"
        );
    }
    // The irrelevant PRs advanced nowhere, and nothing invented drafts.
    for number in ["app-c#301", "app-a#102", "app-b#202", "app-d#401"] {
        assert!(
            !rig.workspace
                .join(format!("run-ledger/drafts/{number}.md"))
                .exists(),
            "PR {number} is irrelevant and has no stage-2 package"
        );
    }
    // Every PR's own rollup section: the life ends in the PR's own
    // section, and the seal composes from them — not a window phase
    // inventing one.
    for number in [
        "app-a#101",
        "app-b#201",
        "app-c#301",
        "app-a#102",
        "app-b#202",
        "app-d#401",
    ] {
        assert!(
            rig.workspace
                .join(format!("run-ledger/rollup/{number}.md"))
                .is_file(),
            "PR {number} has its own rollup section"
        );
    }
    // Every child the run launched also settled: the drain holds even
    // though the driver path here skips production's run-end drain.
    let (started, finished, _peak) = spawn_counts(&rig.events);
    assert_eq!(
        started, 17,
        "the nested tree is exactly 17 children: {started}"
    );
    assert_eq!(started, finished, "no child is left dangling");
}

#[test]
fn a_pipeline_spawns_its_own_children_under_a_saturated_pool() {
    // The per-PR-life proof at saturation: with only two slots, the
    // orchestrator's pipeline dispatch queues, each pipeline spawns its
    // own stage children as continuations, and every PR's life still
    // completes — a stage child never waits behind pipelines that have
    // not begun, and no spawn is ever refused for a full pool.
    let rig = rig_for("nested");
    let report = run_simulation(&rig).expect("the simulated workflow settles");
    assert_eq!(report.exit_code, 0, "{report:?}");

    // Every one of the six pipelines ran to its section: a queueing
    // pool and continuation priority delivered all six whole lives.
    for number in [
        "app-a#101",
        "app-b#201",
        "app-c#301",
        "app-a#102",
        "app-b#202",
        "app-d#401",
    ] {
        assert!(
            rig.workspace
                .join(format!("run-ledger/rollup/{number}.md"))
                .is_file(),
            "PR {number}'s whole life completed under saturation"
        );
    }
    // Depth held and the tree is complete: six pipelines, each with its
    // classifier; two actionable PRs with a drafter and a committer
    // each; the seal's one rollup child. Lease calls are bash, not
    // children. Exactly that tree — nothing extra, nothing dangling.
    let (started, finished, peak) = spawn_counts(&rig.events);
    assert_eq!(
        started, 17,
        "the nested tree is exactly 17 children: {started}"
    );
    assert_eq!(started, finished, "no child is left dangling");
    // The two-slot cap bounds WORKING children; a child waiting in a
    // poll yields its slot to its own queued stage children, so the
    // physical peak is the cap plus the waiters — bounded, never a
    // serial run.
    assert!(
        peak <= 8,
        "the tree stayed bounded by cap plus waiters: peak {peak}"
    );
}

/// Reads the drained event stream: (children launched, children settled,
/// peak concurrent children) over the whole run.
fn spawn_counts(
    events: &std::sync::mpsc::Receiver<Option<craft_runtime::RuntimeEvent>>,
) -> (usize, usize, usize) {
    let mut started = 0usize;
    let mut finished = 0usize;
    let mut alive: usize = 0;
    let mut peak = 0usize;
    while let Ok(event) = events.try_recv() {
        match event {
            Some(craft_runtime::RuntimeEvent::SubagentStarted { .. }) => {
                started += 1;
                alive += 1;
                if alive > peak {
                    peak = alive;
                }
            }
            Some(craft_runtime::RuntimeEvent::SubagentFinished { .. }) => {
                finished += 1;
                alive = alive.saturating_sub(1);
            }
            _ => {}
        }
    }
    (started, finished, peak)
}

#[test]
fn the_simulation_fails_closed_when_the_hub_cannot_mount() {
    // A hostile hub setting must abort the run rather than silently
    // degrade to serial work. The cap parser is the seam that refuses,
    // so the test asserts its typed error (no env mutation needed).
    let hostile = craft_runtime::SubagentCaps::resolve(
        Some(String::from("4")),
        Some(String::from("banana")),
        Some(String::from("4")),
    );
    assert!(
        hostile.is_err(),
        "a hostile cap knob is a typed error, never a silent default"
    );
    let detail = hostile.expect_err("hostile knob errors");
    assert!(
        detail.contains("banana"),
        "the error names the hostile value: {detail}"
    );
}
