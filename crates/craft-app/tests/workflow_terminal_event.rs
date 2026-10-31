//! RED corpus: the workflow's terminal stream contract — a full event
//! queue must not cost the `workflow_finished` line.

mod workflow_common;

use std::collections::BTreeMap;
use std::fs;
use std::sync::mpsc;

use craft_app::RuntimeEvent;
use craft_app::cli::OutputFormat;
use craft_app::event_writer::{self, EVENT_QUEUE_CAP, EventSink};
use craft_app::workflow_cmd::{drive_workflow, resolve_step_agents, workflow_run_dir};
use craft_app::workflow_exec::{StepMount, WorkflowExecutor};
use craft_core::agents::AgentCatalog;
use craft_runtime::{SubagentCaps, SubagentHub, SubagentSettings, SubagentWiring};
use craft_tools::ToolLimits;
use craft_workflow::run::{args_digest, run_id_for};
use craft_workflow::{Journal, WorkflowPlan, plan};

use workflow_common::{FakeProvider, repo, text_reply, usage, write_step_agent};

const ONE_STEP_PLAN: &str = r#"---
name: one-step
steps:
  - id: only
    agent: step-agent
    max-turns: 3
    retry: 0
    on-failure: stop
    body: |
      Do the one step.
---
"#;

/// The run's hub, mounted as production mounts it: the executor owns
/// one clone and `drive_workflow` takes its own, so a shared pool
/// spans the whole run and the terminal drain can settle it.
fn terminal_hub(
    sessions_dir: std::path::PathBuf,
    provider: std::sync::Arc<FakeProvider>,
) -> SubagentWiring {
    let agent = workflow_common::step_agent_profile();
    let settings = SubagentSettings {
        caps: SubagentCaps {
            max_depth: 3,
            max_total: 16,
            max_concurrency: 4,
        },
        run_token_cap: None,
        per_request_tokens: 4_096,
        context: craft_runtime::ContextKnobs::default(),
        default_system_prompt: String::new(),
        catalog: std::sync::Arc::new(AgentCatalog {
            agents: vec![agent],
            skipped: Vec::new(),
        }),
        limits: ToolLimits::default(),
        workspace_root: sessions_dir.parent().unwrap_or(&sessions_dir).to_path_buf(),
        parent_session: Some(sessions_dir.join("subagents.jsonl")),
        idle_timeout: None,
        step_attempts: None,
        step_patience: None,
    };
    SubagentWiring::root(SubagentHub::mounting(settings, provider))
}

#[test]
fn workflow_finished_waits_out_a_full_queue_instead_of_dropping() {
    let workspace = repo("wf-terminal-queue").expect("workspace");
    write_step_agent(&workspace).expect("profile written");
    let plan = WorkflowPlan::parse("plan.md", ONE_STEP_PLAN).expect("plan parses");
    let args = plan::resolve_args(&plan, &BTreeMap::new()).expect("args resolve");
    let digest = args_digest(&args).expect("digest");
    let run_dir = workflow_run_dir(&workspace, &run_id_for(&plan.plan_hash, &digest));
    fs::create_dir_all(run_dir.join("sessions")).expect("run dir");
    let agents = resolve_step_agents(&plan, &workspace).expect("profiles resolve");
    let provider = FakeProvider::new(vec![text_reply("done", usage(3, 1))]);
    let hub_provider = provider.clone();
    let mount: StepMount<FakeProvider> = std::sync::Arc::new(move |_model| Ok(provider.clone()));
    let executor = WorkflowExecutor::new(
        mount,
        "base-model".to_string(),
        workspace.clone(),
        run_dir.join("sessions"),
        Some(plan.limits.max_total_tokens),
        None,
        Some(3),
        agents,
        craft_runtime::ContextKnobs::default(),
    );
    let journal = Journal::open(&run_dir.join("journal.jsonl")).expect("journal");
    let (tx, events) = mpsc::sync_channel(EVENT_QUEUE_CAP);
    let (closed, _closed_rx) = tokio::sync::watch::channel(false);
    let sink = EventSink::from_raw_parts(tx, closed);
    for _ in 0..EVENT_QUEUE_CAP {
        sink.emit(RuntimeEvent::TurnStarted { turn: 1 });
    }
    let (_writer_sink, writer) =
        event_writer::spawn_event_writer(OutputFormat::StreamJson).expect("writer spawns");
    let hub = terminal_hub(run_dir.join("sessions"), hub_provider);
    let drive = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime builds");
        runtime.block_on(drive_workflow(
            &plan, &args, &executor, journal, &sink, writer, hub,
        ))
    });
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(
        !drive.is_finished(),
        "the terminal workflow event must wait for queue room, not drop"
    );
    for _ in 0..EVENT_QUEUE_CAP {
        drop(events.recv());
    }
    let tail = events
        .recv()
        .expect("the terminal event lands once room opens");
    assert!(
        matches!(
            tail,
            Some(RuntimeEvent::WorkflowFinished { exit_code: 0, .. })
        ),
        "the queued terminal event is the workflow_finished line"
    );
    let report = drive.join().expect("drive joins").expect("workflow runs");
    assert_eq!(report.exit_code, 0);
}
