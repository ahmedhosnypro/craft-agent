//! The simulation rig: one workspace carrying the 8-step plan, the
//! profile map the executor resolves each step against, the hub catalog
//! the spawn calls resolve against, and the event stream the spawn
//! proofs read.

use std::collections::BTreeMap;
use std::sync::Arc;

use craft_app::event_writer::EventSink;
use craft_app::workflow_cmd::workflow_run_dir;
use craft_core::agents::{AgentCatalog, AgentFile, PromptMode};
use craft_runtime::{SubagentCaps, SubagentHub, SubagentSettings, SubagentWiring};
use craft_tools::ToolLimits;
use craft_workflow::run::{args_digest, run_id_for};
use craft_workflow::{WorkflowPlan, plan};

use crate::scripted::SimProvider;

pub struct Rig {
    pub workspace: std::path::PathBuf,
    pub run_dir: std::path::PathBuf,
    pub agents: BTreeMap<String, AgentFile>,
    pub provider: Arc<SimProvider>,
    pub hub: SubagentWiring,
    /// The run's event stream: every subagent launch and settle lands
    /// here, so the spawn proofs read the live feed, not transcripts.
    pub sink: Option<EventSink>,
    pub events: std::sync::mpsc::Receiver<Option<craft_runtime::RuntimeEvent>>,
}

fn profile(name: &str, tools: &[&str]) -> AgentFile {
    AgentFile {
        path: Default::default(),
        name: name.to_string(),
        description: String::from("simulation profile"),
        model: None,
        prompt_mode: PromptMode::Full,
        tools: Some(tools.iter().map(|tool| tool.to_string()).collect()),
        disallowed_tools: Vec::new(),
        deny_read: Vec::new(),
        deny_write: Vec::new(),
        max_turns: None,
        timeout_secs: None,
        background: false,
        agents_md: false,
        body: String::from("You are a simulation step."),
    }
}

pub fn rig(name: &str) -> Rig {
    let workspace = std::env::temp_dir().join(format!(
        "craft-sim-{}-{name}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&workspace).expect("workspace");
    std::fs::create_dir_all(workspace.join("sessions")).expect("sessions dir");
    // The provider reads the workspace: the simulated orchestrator learns
    // which PRs still need their stage-2 child from the ledger files.
    let provider = SimProvider::new();
    let spawn_tools = [
        "bash",
        "read_file",
        "write_file",
        "spawn_agent",
        "spawn_agents",
        "poll_agents",
    ];
    let read_write = ["read_file", "write_file"];
    let read_only = ["read_file"];
    let mut agents = BTreeMap::new();
    agents.insert(
        "prepare".to_string(),
        profile("sim-orchestrator", &spawn_tools),
    );
    agents.insert(
        "pr-pipeline".to_string(),
        profile("sim-orchestrator", &spawn_tools),
    );
    agents.insert(
        "pipeline-gate".to_string(),
        profile("sim-judge", &read_only),
    );
    agents.insert(
        "pipeline-appeal".to_string(),
        profile("sim-judge", &read_only),
    );
    agents.insert(
        "rollup-seal".to_string(),
        profile("sim-orchestrator", &spawn_tools),
    );
    agents.insert("rollup-gate".to_string(), profile("sim-judge", &read_only));
    agents.insert(
        "rollup-appeal".to_string(),
        profile("sim-judge", &read_only),
    );
    agents.insert(
        "rollup-commit".to_string(),
        profile("sim-committer", &read_write),
    );
    // The specialists the dispatch and the per-PR pipelines spawn: the
    // catalog the hub resolves a spawn call's profile against is the
    // same one. A sim-pipeline child spawns its own stage children at
    // depth 2, and those spawn nothing — the depth-2 ceiling.
    agents.insert(
        "pipeline".to_string(),
        profile("sim-pipeline", &spawn_tools),
    );
    agents.insert("cloner".to_string(), profile("sim-cloner", &read_write));
    agents.insert(
        "classifier".to_string(),
        profile("sim-classifier", &read_write),
    );
    agents.insert("filer".to_string(), profile("sim-filer", &read_write));
    agents.insert("applier".to_string(), profile("sim-applier", &read_write));
    agents.insert(
        "filing-committer".to_string(),
        profile("sim-filing-committer", &read_write),
    );
    agents.insert(
        "bump-committer".to_string(),
        profile("sim-bump-committer", &read_write),
    );
    agents.insert("rollup".to_string(), profile("sim-rollup", &read_write));
    let catalog = AgentCatalog {
        agents: agents.values().cloned().collect(),
        skipped: Vec::new(),
    };
    let settings = SubagentSettings {
        // Tight caps on purpose: the per-PR-life architecture must be
        // exercised at saturation every run, not only at window scale.
        // A detached spawn queues on a full pool instead of refusing,
        // and a running child's dispatch launches ahead of fresh ones.
        caps: SubagentCaps {
            max_depth: 2,
            max_total: 512,
            max_concurrency: 2,
        },
        run_token_cap: None,
        per_request_tokens: 4_096,
        default_system_prompt: String::new(),
        catalog: Arc::new(catalog),
        limits: ToolLimits::default(),
        workspace_root: workspace.clone(),
        parent_session: None,
        idle_timeout: None,
        step_attempts: None,
        step_patience: None,
    };
    let hub = SubagentWiring::root(SubagentHub::mounting(settings, provider.clone()));
    let plan_text = include_str!("plan.md");
    std::fs::create_dir_all(workspace.join(".craft-agent").join("workflows")).expect("wf dir");
    std::fs::write(
        workspace
            .join(".craft-agent")
            .join("workflows")
            .join("sim-adopt.md"),
        plan_text,
    )
    .expect("plan file");
    let plan_spec = WorkflowPlan::parse_file(
        &workspace
            .join(".craft-agent")
            .join("workflows")
            .join("sim-adopt.md"),
    )
    .expect("plan parses");
    let args = plan::resolve_args(&plan_spec, &BTreeMap::new()).expect("args");
    let run_id = run_id_for(&plan_spec.plan_hash, &args_digest(&args).expect("digest"));
    let run_dir = workflow_run_dir(&workspace, &run_id);
    std::fs::create_dir_all(run_dir.join("sessions")).expect("run sessions dir");
    let (tx, events) = std::sync::mpsc::sync_channel(4096);
    let (closed, _closed_rx) = tokio::sync::watch::channel(false);
    let sink = EventSink::from_raw_parts(tx, closed);
    Rig {
        workspace,
        run_dir,
        agents,
        provider,
        hub,
        sink: Some(sink),
        events,
    }
}
