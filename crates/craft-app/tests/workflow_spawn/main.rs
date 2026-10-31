//! Workflow steps over a mounted subagent hub: the spawn pair is
//! reachable by a profile whose allowlist carries it, the step awaits
//! the child as its own turn, and the pair stays out of a registry
//! that does not list it — with or without the wiring behind it.

mod detached;
#[path = "../workflow_common/mod.rs"]
mod workflow_common;

use std::{collections::BTreeMap, sync::Arc};

use craft_app::workflow_exec::{StepMount, WorkflowExecutor};
use craft_core::agents::{AgentCatalog, AgentFile};
use craft_runtime::{SubagentCaps, SubagentHub, SubagentSettings, SubagentWiring};
use craft_tools::ToolLimits;
use craft_workflow::status::StepConclusion;
use craft_workflow::{StepExecutor, StepInputs};

use workflow_common::{FakeProvider, scratch, step_agent_profile, text_reply, tool_reply, usage};

/// A profile whose allowlist carries the spawn pair, like the scan's
/// orchestrator and classifier profiles.
fn spawn_agent_profile() -> AgentFile {
    AgentFile {
        tools: Some(vec![
            "read_file".to_string(),
            "spawn_agent".to_string(),
            "spawn_agents".to_string(),
            "poll_agents".to_string(),
        ]),
        ..step_agent_profile()
    }
}

/// The run hub the production wiring mounts, mounted here over the
/// fake provider with a scratch catalog.
fn hub_over(workspace_root: std::path::PathBuf, provider: Arc<FakeProvider>) -> SubagentWiring {
    let catalog = AgentCatalog {
        agents: vec![step_agent_profile()],
        skipped: Vec::new(),
    };
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
        catalog: Arc::new(catalog),
        limits: ToolLimits::default(),
        workspace_root,
        parent_session: None,
        idle_timeout: None,
        step_attempts: None,
        step_patience: None,
    };
    SubagentWiring::root(SubagentHub::mounting(settings, provider))
}

struct Rig {
    workspace: std::path::PathBuf,
}

fn rig(name: &str) -> std::io::Result<Rig> {
    let workspace = scratch(name)?;
    std::fs::create_dir_all(workspace.join("sessions"))?;
    Ok(Rig { workspace })
}

fn inputs() -> StepInputs {
    StepInputs {
        id: "s1".to_string(),
        agent: workflow_common::AGENT_NAME.to_string(),
        model: None,
        max_turns: 25,
        seat: None,
        feedback: Vec::new(),
        deny_read: Vec::new(),
        deny_write: Vec::new(),
        prompt: "Run the step.".to_string(),
    }
}

fn inputs_turns(max_turns: u32) -> StepInputs {
    StepInputs {
        max_turns,
        ..inputs()
    }
}
fn inputs_seat(seat: u32) -> StepInputs {
    StepInputs {
        seat: Some(seat),
        ..inputs()
    }
}

/// The executor over one pre-resolved profile; the step's own agent
/// map and the hub's scratch catalog are both in-memory.
fn executor_over(
    rig: &Rig,
    provider: Arc<FakeProvider>,
    profile: AgentFile,
) -> WorkflowExecutor<FakeProvider> {
    let agents = BTreeMap::from([("s1".to_string(), profile)]);
    let mount: StepMount<FakeProvider> = Arc::new(move |_model| Ok(provider.clone()));
    WorkflowExecutor::new(
        mount,
        "base-model".to_string(),
        rig.workspace.clone(),
        rig.workspace.join("sessions"),
        Some(4_000_000),
        None,
        Some(3),
        agents,
        craft_runtime::ContextKnobs::default(),
    )
}

/// A step whose profile allowlists the pair, driven over a mounted
/// hub: the spawn call reaches a real child (three provider turns
/// total — the step's tool call, the child's reply, the step's close),
/// and the child's final message comes back as the tool result.
#[tokio::test]
async fn a_wired_step_spawns_and_awaits_the_child() {
    let provider = FakeProvider::new(vec![
        tool_reply(
            "spawn-1",
            "spawn_agent",
            r#"{"agent":"step-agent","prompt":"classify this","maxTurns":5}"#,
            usage(2, 1),
        ),
        text_reply("child verdict", usage(1, 1)),
        text_reply("step done", usage(1, 1)),
    ]);
    let rig = rig("spawn-wired").expect("rig");
    let hub = hub_over(rig.workspace.clone(), provider.clone());
    let executor =
        executor_over(&rig, provider.clone(), spawn_agent_profile()).with_subagents(Some(hub));
    let outcome = executor.execute(inputs()).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);
    assert_eq!(outcome.final_message, "step done");
    assert_eq!(provider.calls(), 3, "the child ran as its own turn");
}

/// The same step without the hub keeps the spawn-less registry: the
/// tool call is refused, the child never runs, and the step settles
/// on its next turn instead of hanging.
#[tokio::test]
async fn an_unwired_step_cannot_spawn() {
    let provider = FakeProvider::new(vec![
        tool_reply(
            "spawn-1",
            "spawn_agent",
            r#"{"agent":"step-agent","prompt":"classify this"}"#,
            usage(2, 1),
        ),
        text_reply("no hub", usage(1, 1)),
    ]);
    let rig = rig("spawn-unwired").expect("rig");
    let executor = executor_over(&rig, provider, spawn_agent_profile());
    let outcome = executor.execute(inputs()).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);
    assert_eq!(outcome.final_message, "no hub");
}

/// The judge-style profile (allowlist without the pair) wired over the
/// same hub still gets no spawn tools: the registry gate is the
/// profile's, not the wiring's.
#[tokio::test]
async fn a_wired_judge_profile_still_cannot_spawn() {
    let provider = FakeProvider::new(vec![
        tool_reply(
            "spawn-1",
            "spawn_agent",
            r#"{"agent":"step-agent","prompt":"re-verify"}"#,
            usage(2, 1),
        ),
        text_reply("judged readonly", usage(1, 1)),
    ]);
    let rig = rig("spawn-judge").expect("rig");
    let hub = hub_over(rig.workspace.clone(), provider.clone());
    let executor = executor_over(&rig, provider, step_agent_profile()).with_subagents(Some(hub));
    let outcome = executor.execute(inputs()).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);
    assert_eq!(outcome.final_message, "judged readonly");
}

/// The step's own turn budget bounds the step run, never the child's:
/// the child runs to its own completion under the hub's caps.
#[tokio::test]
async fn the_child_finishes_even_when_the_step_has_one_turn_left() {
    let provider = FakeProvider::new(vec![
        tool_reply(
            "spawn-1",
            "spawn_agent",
            r#"{"agent":"step-agent","prompt":"run long"}"#,
            usage(2, 1),
        ),
        text_reply("child landed", usage(1, 1)),
        text_reply("wrapped", usage(1, 1)),
    ]);
    let rig = rig("spawn-childbudget").expect("rig");
    let hub = hub_over(rig.workspace.clone(), provider.clone());
    let executor = executor_over(&rig, provider, spawn_agent_profile()).with_subagents(Some(hub));
    let outcome = executor.execute(inputs_turns(2)).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);
    assert_eq!(outcome.final_message, "wrapped");
}

/// The step's subagent events ride the run's event sink: a fan-out is
/// visible in the live stream (and the renderer) while it runs, not
/// only inside the step's own transcript.
#[tokio::test]
async fn a_step_fanout_streams_live_on_the_run_sink() {
    use craft_app::event_writer::{EVENT_QUEUE_CAP, EventSink};
    use craft_runtime::RuntimeEvent;

    let provider = FakeProvider::new(vec![
        tool_reply(
            "spawn-1",
            "spawn_agent",
            r#"{"agent":"step-agent","prompt":"classify this"}"#,
            usage(2, 1),
        ),
        text_reply("child verdict", usage(1, 1)),
        text_reply("step done", usage(1, 1)),
    ]);
    let rig = rig("spawn-sink").expect("rig");
    let hub = hub_over(rig.workspace.clone(), provider.clone());
    let (tx, events) = std::sync::mpsc::sync_channel(EVENT_QUEUE_CAP);
    let (closed, _closed_rx) = tokio::sync::watch::channel(false);
    let sink = EventSink::from_raw_parts(tx, closed);
    let executor = executor_over(&rig, provider, spawn_agent_profile())
        .with_subagents(Some(hub))
        .with_sink(Some(sink));
    let outcome = executor.execute(inputs()).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);

    let mut started = 0;
    let mut finished = 0;
    while let Ok(event) = events.try_recv() {
        match event {
            Some(RuntimeEvent::SubagentStarted { .. }) => started += 1,
            Some(RuntimeEvent::SubagentFinished { .. }) => finished += 1,
            _ => {}
        }
    }
    assert_eq!(started, 1, "the child launch reached the run sink");
    assert_eq!(finished, 1, "the child settle reached the run sink");
}

/// A panel's seats share the step id but not the transcript: each
/// seat's session file is distinct, so three concurrent seats never
/// interleave into one file and a crash-resume replays them apart.
#[tokio::test]
async fn a_panels_seats_keep_distinct_session_files() {
    let provider = FakeProvider::new(vec![text_reply("seat ok", usage(1, 1))]);
    let rig = rig("spawn-seats").expect("rig");
    let executor = executor_over(&rig, provider, step_agent_profile());
    for seat in [0, 1, 2] {
        let outcome = executor
            .execute(inputs_seat(seat))
            .await
            .expect("seat runs");
        assert_eq!(outcome.conclusion, StepConclusion::Success);
    }
    for (seat, name) in [
        (0u32, "s1-seat1.jsonl"),
        (1, "s1-seat2.jsonl"),
        (2, "s1-seat3.jsonl"),
    ] {
        let path = rig.workspace.join("sessions").join(name);
        assert!(path.is_file(), "seat {} lands in {name}", seat + 1);
    }
    assert!(
        !rig.workspace.join("sessions").join("s1.jsonl").is_file(),
        "the un-suffixed step file is never written by a seat"
    );
}
