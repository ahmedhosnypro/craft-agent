//! Child run preparation: the one preparer both spawn paths go
//! through — persona system prompt (profile body per promptMode over
//! the base prompt, plus the fixed subagent overlay), the
//! profile-filtered registry with the spawn pair only when the depth
//! gate and the profile admit it, and the run config. The workflow
//! step executor drives the same preparer, so a step and a spawned
//! child cannot drift. The erased factory turns a prepared child
//! into a future any tool stream can poll.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use craft_core::agents::{AgentFile, PromptMode};
use craft_core::provider::Provider;
use craft_tools::{Registry, ToolLimits};

use crate::envelope::ChildReport;
use crate::events::RuntimeEvent;
use crate::runner::{RunConfig, Runner};
use crate::subagent::SubagentWiring;

pub type ChildFuture = Pin<Box<dyn Future<Output = ChildReport> + Send>>;
pub type ChildFactory = Arc<dyn Fn(ChildRun, String) -> ChildFuture + Send + Sync>;

/// Per-child run inputs: identity, tool limits, run limits, and the
/// child's own spawn wiring (set for spawned children, absent for
/// workflow steps).
pub struct ChildKnobs {
    pub id: String,
    pub workspace_root: PathBuf,
    pub max_turns: u32,
    pub max_total_tokens: Option<u64>,
    pub per_request_tokens: u32,
    /// CLI > env context knobs; the child profile's frontmatter sits
    /// between them and the env in precedence.
    pub context: crate::limits::ContextKnobs,
    pub limits: ToolLimits,
    pub session_path: Option<PathBuf>,
    pub idle_timeout: Option<Duration>,
    pub step_attempts: Option<u32>,
    pub step_patience: Option<Duration>,
    pub spawn: Option<SubagentWiring>,
    pub deny_read: Vec<String>,
    pub deny_write: Vec<String>,
}

/// A prepared child run: the resolved config and the filtered
/// registry, ready for any `Runner` to drive.
pub struct ChildRun {
    pub id: String,
    pub config: RunConfig,
    pub registry: Registry,
}

/// Prepares one child run. `overlay` is set for spawned children and
/// left out for workflow steps; `knobs.spawn` carries the child's own
/// wiring so its registry can hold the spawn pair one level deeper
/// while the depth gate stays honest.
pub fn prepare_child(
    profile: &AgentFile,
    default_prompt: &str,
    overlay: Option<&str>,
    knobs: ChildKnobs,
) -> Result<ChildRun, String> {
    let spawn = knobs.spawn.filter(crate::subagent_admit::spawn_admits);
    let registry = crate::profile::build_registry(Some(profile), knobs.limits, spawn.clone());
    let mut system = match profile.prompt_mode {
        PromptMode::Full => profile.body.clone(),
        PromptMode::Extend => format!("{default_prompt}\n\n{}", profile.body),
    };
    if let Some(overlay) = overlay {
        system.push_str("\n\n");
        system.push_str(overlay);
    }
    let mut config = RunConfig::new(knobs.workspace_root);
    config.system = Some(system);
    config.max_turns = knobs.max_turns;
    config.max_total_tokens = knobs.max_total_tokens;
    config.max_tokens = knobs.per_request_tokens;
    crate::limits::ContextKnobs::resolve(
        &knobs.context,
        &crate::limits::ContextKnobs::from_agent(Some(profile)),
        &crate::limits::ContextKnobs::default(),
    )
    .apply(&mut config)?;
    config.idle_timeout = knobs.idle_timeout;
    config.step_attempts = knobs.step_attempts;
    config.step_patience = knobs.step_patience;
    config.session_path = knobs.session_path;
    config.subagents = spawn;
    config.deny_read = knobs.deny_read;
    config.deny_write = knobs.deny_write;
    Ok(ChildRun {
        id: knobs.id,
        config,
        registry,
    })
}

/// The erased child factory: one full child `Runner` over the parent
/// backend, its event stream folded into a final-message report —
/// per-turn spend and turn milestones noted into the hub, child
/// per-turn events never surfaced to the parent stream.
pub(crate) fn runner_factory<P: Provider + Send + Sync + 'static>(
    provider: Arc<P>,
    hub: Arc<crate::subagent::SubagentHub>,
) -> ChildFactory {
    Arc::new(move |child: ChildRun, prompt: String| {
        let provider = provider.clone();
        let hub = hub.clone();
        Box::pin(async move {
            let ChildRun {
                id,
                config,
                registry,
            } = child;
            let runner = Runner::new(provider, registry, config);
            let mut final_text = String::new();
            let outcome = runner
                .run(&prompt, |event| match event {
                    RuntimeEvent::TurnStarted { turn } => {
                        final_text.clear();
                        hub.note_child_turn(&id, turn);
                    }
                    RuntimeEvent::TurnRetried { .. } => final_text.clear(),
                    RuntimeEvent::AssistantDelta { text, .. } => final_text.push_str(&text),
                    RuntimeEvent::TurnUsage { usage, .. } => hub.note_child_usage(&id, usage),
                    _ => {}
                })
                .await;
            ChildReport::from_outcome(outcome, final_text)
        })
    })
}
