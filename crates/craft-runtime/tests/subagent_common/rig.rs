//! The run rig: scratch workspaces, in-memory profile fixtures,
//! hub mounting, and parent-run helpers with event collectors.
#![allow(dead_code)]
#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use craft_core::agents::{AgentCatalog, AgentFile, PromptMode};

use super::provider::FakeProvider;

/// A unique scratch workspace per test row.
pub fn workspace(name: &str) -> std::io::Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "craft-runtime-subagent-{}-{name}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// One fixture profile, no disk involved.
fn profile(
    name: &str,
    tools: Option<Vec<&str>>,
    timeout_secs: Option<u64>,
    body: &str,
) -> AgentFile {
    AgentFile {
        path: PathBuf::from(format!("{name}.md")),
        name: name.to_string(),
        description: format!("Runs when a subagent row needs {name}."),
        model: None,
        prompt_mode: PromptMode::Full,
        tools: tools.map(|list| list.into_iter().map(str::to_string).collect()),
        disallowed_tools: Vec::new(),
        deny_read: Vec::new(),
        deny_write: Vec::new(),
        max_turns: None,
        timeout_secs,
        max_context_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        thinking: None,
        background: false,
        agents_md: false,
        body: body.to_string(),
    }
}

/// The read-only worker most rows spawn.
pub fn scout() -> AgentFile {
    profile(
        "repo-scout",
        Some(vec!["read_file", "glob", "grep"]),
        None,
        "You are a repo scout. Answer briefly.",
    )
}

/// A writer, for the clobber row.
pub fn writer() -> AgentFile {
    profile(
        "docs-writer",
        Some(vec!["read_file", "write_file", "edit_file"]),
        None,
        "You are a docs writer.",
    )
}

/// A profile that may spawn, for the depth row.
pub fn spawner() -> AgentFile {
    profile(
        "pr-analyst",
        Some(vec!["spawn_agent", "read_file"]),
        None,
        "You orchestrate subagents.",
    )
}

/// A pipeline child: it spawns its own stage children and polls for
/// them, for the nested-queue rows.
pub fn pipeline() -> AgentFile {
    profile(
        "pr-pipeline",
        Some(vec![
            "spawn_agent",
            "spawn_agents",
            "poll_agents",
            "read_file",
        ]),
        None,
        "You run one PR's pipeline.",
    )
}

/// A slow profile whose wall clock is 1 second.
pub fn slowpoke() -> AgentFile {
    profile(
        "slowpoke",
        Some(vec!["read_file"]),
        Some(1),
        "You take your time.",
    )
}

/// The catalog of fixture profiles, keyed by name.
pub fn catalog(agents: Vec<AgentFile>) -> Arc<AgentCatalog> {
    Arc::new(AgentCatalog {
        agents,
        skipped: Vec::new(),
    })
}

/// The run rig: workspace, parent session, hub, and root wiring.
pub struct Rig {
    pub workspace: PathBuf,
    pub session: Option<PathBuf>,
    pub hub: Arc<craft_runtime::SubagentHub>,
    pub wiring: craft_runtime::SubagentWiring,
}

/// The knobs a corpus row sets for its rig.
pub struct RigSpec {
    name: String,
    caps: craft_runtime::SubagentCaps,
    token_cap: u64,
    parent_session: bool,
}

impl RigSpec {
    /// In-memory parent transcripts.
    pub fn memory(name: &str, token_cap: u64) -> Self {
        Self {
            name: name.to_string(),
            caps: craft_runtime::SubagentCaps::default(),
            token_cap,
            parent_session: false,
        }
    }

    /// File-backed parent transcripts.
    pub fn logged(name: &str, token_cap: u64) -> Self {
        Self {
            name: name.to_string(),
            caps: craft_runtime::SubagentCaps::default(),
            token_cap,
            parent_session: true,
        }
    }

    /// Overrides the recursion caps.
    pub fn caps(mut self, caps: craft_runtime::SubagentCaps) -> Self {
        self.caps = caps;
        self
    }
}

/// Mounts the hub over the fake provider with the spec's caps, token
/// budget, and transcript mode.
pub fn rig(spec: RigSpec, provider: Arc<FakeProvider>, agents: Vec<AgentFile>) -> Rig {
    let workspace = workspace(&spec.name).expect("scratch dir");
    let session = spec.parent_session.then(|| {
        let path = workspace.join("sessions").join("parent.jsonl");
        std::fs::create_dir_all(path.parent().expect("sessions dir")).expect("sessions dir");
        path
    });
    let settings = craft_runtime::SubagentSettings {
        caps: spec.caps,
        run_token_cap: Some(spec.token_cap),
        per_request_tokens: 1024,
        context: craft_runtime::ContextKnobs::default(),
        default_system_prompt: String::from("Base prompt."),
        catalog: catalog(agents),
        limits: craft_tools::ToolLimits::default(),
        workspace_root: workspace.clone(),
        parent_session: session.clone(),
        idle_timeout: None,
        step_attempts: Some(3),
        step_patience: Some(std::time::Duration::ZERO),
    };
    let hub = craft_runtime::SubagentHub::mounting(settings, provider);
    let wiring = craft_runtime::SubagentWiring::root(hub.clone());
    Rig {
        workspace,
        session,
        hub,
        wiring,
    }
}

/// The parent run config and registry: all builtins plus the spawn
/// pair, the run token cap, and the parent session.
pub fn parent_run(rig: &Rig, max_turns: u32) -> (craft_runtime::RunConfig, craft_tools::Registry) {
    let mut config = craft_runtime::RunConfig::new(rig.workspace.clone());
    config.max_turns = max_turns;
    config.max_total_tokens = rig.hub.settings().run_token_cap;
    config.session_path = rig.session.clone();
    config.subagents = Some(rig.wiring.clone());
    let registry = craft_runtime::profile::build_registry(
        None,
        craft_tools::ToolLimits::default(),
        Some(rig.wiring.clone()),
    );
    (config, registry)
}

/// Drives the runner, collecting the parent event stream.
pub async fn run(
    config: craft_runtime::RunConfig,
    registry: craft_tools::Registry,
    provider: Arc<FakeProvider>,
    prompt: &str,
) -> (craft_runtime::RunOutcome, Vec<craft_runtime::RuntimeEvent>) {
    let runner = craft_runtime::Runner::new(provider, registry, config);
    let mut events = Vec::new();
    let outcome = runner.run(prompt, |event| events.push(event)).await;
    (outcome, events)
}

/// The tool-result contents of one run, in order.
pub fn tool_results(events: &[craft_runtime::RuntimeEvent]) -> Vec<(bool, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            craft_runtime::RuntimeEvent::ToolResult {
                is_error, content, ..
            } => Some((*is_error, content.clone())),
            _ => None,
        })
        .collect()
}

/// The (id, profile) pairs of the subagent_started events, in order.
pub fn started(events: &[craft_runtime::RuntimeEvent]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            craft_runtime::RuntimeEvent::SubagentStarted { id, profile, .. } => {
                Some((id.clone(), profile.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The (id, stop_reason, turns) triples of the finished events, in
/// settle order.
pub fn finished(events: &[craft_runtime::RuntimeEvent]) -> Vec<(String, String, u32)> {
    events
        .iter()
        .filter_map(|event| match event {
            craft_runtime::RuntimeEvent::SubagentFinished {
                id,
                stop_reason,
                turns,
                ..
            } => Some((id.clone(), stop_reason.clone(), *turns)),
            _ => None,
        })
        .collect()
}
