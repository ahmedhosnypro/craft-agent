//! The workflow step executor: one bounded runtime run per step over the
//! pre-resolved agent profile, mapping the run outcome onto the engine's
//! step vocabulary.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use craft_core::agents::AgentFile;
use craft_core::provider::Provider;
use craft_runtime::{
    ChildKnobs, ChildRun, ContextKnobs, DEFAULT_REQUEST_TOKENS, ExitStatus, Runner, RuntimeEvent,
    SubagentWiring, prepare_child,
};
use craft_tools::ToolLimits;
use craft_workflow::status::{StepConclusion, StepUsage};
use craft_workflow::{StepExecutor, StepInputs, StepOutcome, WorkflowError};

use crate::event_writer::EventSink;
use crate::prompt::DEFAULT_SYSTEM_PROMPT;

/// Mounts the provider one step runs against, for its model id.
pub type StepMount<P> = Arc<dyn Fn(&str) -> Result<Arc<P>, String> + Send + Sync>;

/// Per-step run knobs the executor injects into every `prepare_child`.
#[derive(Clone)]
struct StepKnobs {
    workspace_root: PathBuf,
    sessions_dir: PathBuf,
    step_token_cap: Option<u64>,
    idle_timeout: Option<Duration>,
    step_attempts: Option<u32>,
    step_patience: Option<Duration>,
    /// CLI > env context knobs; each step profile's frontmatter sits
    /// between them and the env in precedence.
    context: ContextKnobs,
}

/// The production `StepExecutor`: the caller injects the provider
/// mount, the pre-resolved step profiles, and the run-dir paths.
pub struct WorkflowExecutor<P: Provider + Send + Sync> {
    mount: StepMount<P>,
    default_model: String,
    knobs: StepKnobs,
    agents: BTreeMap<String, AgentFile>,
    /// The run's shared subagent wiring: every step's registry holds
    /// the spawn pair through it, while each profile's `tools`
    /// allowlist decides whether that profile may actually use it.
    subagents: Option<SubagentWiring>,
    /// The run's event sink: a step's subagent launches and settles
    /// land on it, so a fan-out is visible in the live stream and the
    /// renderer rather than only inside the step's own transcript.
    sink: Option<EventSink>,
}

impl<P: Provider + Send + Sync> WorkflowExecutor<P> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mount: StepMount<P>,
        default_model: String,
        workspace_root: PathBuf,
        sessions_dir: PathBuf,
        step_token_cap: Option<u64>,
        idle_timeout: Option<Duration>,
        step_attempts: Option<u32>,
        agents: BTreeMap<String, AgentFile>,
        context: ContextKnobs,
    ) -> Self {
        Self {
            mount,
            default_model,
            knobs: StepKnobs {
                workspace_root,
                sessions_dir,
                step_token_cap,
                idle_timeout,
                step_attempts,
                step_patience: None,
                context,
            },
            agents,
            subagents: None,
            sink: None,
        }
    }

    /// Mounts the run's subagent hub over the executor: a step fan-out
    /// needs this. Without it a step keeps today's spawn-less registry.
    pub fn with_subagents(mut self, wiring: Option<SubagentWiring>) -> Self {
        self.subagents = wiring;
        self
    }

    /// Overrides the steps' transient-patience window; None keeps the
    /// env resolution inside each step's own runner.
    pub fn with_step_patience(mut self, patience: Option<Duration>) -> Self {
        self.knobs.step_patience = patience;
        self
    }

    /// Points the executor at the run's event sink: a step's subagent
    /// launches and settles stream live, exactly like a mission run's.
    pub fn with_sink(mut self, sink: Option<EventSink>) -> Self {
        self.sink = sink;
        self
    }
}

impl<P: Provider + Send + Sync> StepExecutor for WorkflowExecutor<P> {
    fn execute(
        &self,
        step: StepInputs,
    ) -> impl Future<Output = Result<StepOutcome, WorkflowError>> + Send {
        let mount = self.mount.clone();
        let default_model = self.default_model.clone();
        let knobs = self.knobs.clone();
        let subagents = self.subagents.clone();
        let sink = self.sink.clone();
        let agent = self.agents.get(&step.id).cloned();
        async move {
            let launch = |detail: String| WorkflowError::StepLaunch {
                step: step.id.clone(),
                detail,
            };
            let Some(agent) = agent else {
                return Err(launch(String::from(
                    "no agent profile resolved for the step",
                )));
            };
            let model = step.model.clone().unwrap_or(default_model);
            let provider = mount(&model).map_err(launch)?;
            let child = prepare_step_child(&agent, &step, &knobs, &subagents).map_err(launch)?;
            let stream = drive_step(provider, child, &step, sink.as_ref()).await;
            Ok(finish_step(stream))
        }
    }
}

/// The step's child run: profile, session, knobs, and the run's own
/// deny lists merged over the profile's. `subagents` carries the run's
/// shared wiring — the step's registry holds the spawn pair through
/// it, gated by the profile's allowlist at registry-build time.
fn prepare_step_child(
    agent: &AgentFile,
    step: &StepInputs,
    knobs: &StepKnobs,
    subagents: &Option<SubagentWiring>,
) -> Result<ChildRun, String> {
    let mut deny_read = agent.deny_read.clone();
    deny_read.extend(step.deny_read.iter().cloned());
    let mut deny_write = agent.deny_write.clone();
    deny_write.extend(step.deny_write.iter().cloned());
    // A judge panel runs its seats over one step id; the seat index
    // keeps each transcript distinct, so a panel's three seats never
    // interleave into one file.
    let session_stem = match step.seat {
        Some(seat) => format!("{}-seat{}", step.id, seat + 1),
        None => step.id.clone(),
    };
    let session_path = knobs.sessions_dir.join(format!("{session_stem}.jsonl"));
    prepare_child(
        agent,
        DEFAULT_SYSTEM_PROMPT,
        None,
        ChildKnobs {
            id: step.id.clone(),
            workspace_root: knobs.workspace_root.clone(),
            max_turns: step.max_turns,
            max_total_tokens: knobs.step_token_cap,
            per_request_tokens: DEFAULT_REQUEST_TOKENS,
            context: knobs.context.clone(),
            limits: ToolLimits::default(),
            session_path: Some(session_path),
            idle_timeout: knobs.idle_timeout,
            step_attempts: knobs.step_attempts,
            step_patience: knobs.step_patience,
            spawn: subagents.clone(),
            deny_read,
            deny_write,
        },
    )
}

/// One step's run: the streamed assistant text preserved per turn, the
/// rest of the terminal outcome carried through.
struct StepStream {
    exit: ExitStatus,
    turns: u32,
    usage: craft_core::types::Usage,
    detail: String,
    final_text: String,
}

/// Runs the step to its conclusion, folding the streamed deltas. A
/// step's subagent launches and settles also ride the run's event
/// sink, so a fan-out is visible in the live stream and the renderer
/// while it runs, exactly like a mission run's.
async fn drive_step<P: Provider + Send + Sync>(
    provider: Arc<P>,
    child: ChildRun,
    step: &StepInputs,
    sink: Option<&EventSink>,
) -> StepStream {
    let runner = Runner::new(provider, child.registry, child.config);
    let mut final_text = String::new();
    let outcome = runner
        .run(&step.prompt, |event| match event {
            RuntimeEvent::TurnStarted { .. } | RuntimeEvent::TurnRetried { .. } => {
                final_text.clear();
            }
            RuntimeEvent::AssistantDelta { text, .. } => final_text.push_str(&text),
            RuntimeEvent::SubagentStarted { .. } | RuntimeEvent::SubagentFinished { .. } => {
                if let Some(sink) = sink {
                    sink.emit(event.clone());
                }
            }
            _ => {}
        })
        .await;
    StepStream {
        exit: outcome.exit,
        turns: outcome.turns,
        usage: outcome.usage,
        detail: outcome.detail,
        final_text,
    }
}

/// The run's terminal record as one step outcome. A successful run
/// always carries text: the fold rejects zero-content turns, so the
/// last turn's text is the handoff.
fn finish_step(stream: StepStream) -> StepOutcome {
    let conclusion = map_conclusion(stream.exit);
    let final_message = if conclusion.is_success() {
        stream.final_text
    } else {
        stream.detail
    };
    StepOutcome {
        conclusion,
        usage: map_usage(stream.turns, &stream.usage),
        final_message,
    }
}

/// ExitStatus and StepConclusion share the run exit taxonomy 1:1.
pub fn map_conclusion(exit: ExitStatus) -> StepConclusion {
    match exit {
        ExitStatus::Ok => StepConclusion::Success,
        ExitStatus::Error => StepConclusion::Error,
        ExitStatus::MaxTurns => StepConclusion::MaxTurns,
        ExitStatus::Stationarity => StepConclusion::Stationarity,
        ExitStatus::Budget => StepConclusion::Budget,
    }
}

/// Turn and token accounting onto the engine's buckets; the run's cache
/// buckets have no step counterpart.
pub fn map_usage(turns: u32, usage: &craft_core::types::Usage) -> StepUsage {
    StepUsage {
        turns: u64::from(turns),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_tokens: usage.reasoning_tokens,
    }
}
