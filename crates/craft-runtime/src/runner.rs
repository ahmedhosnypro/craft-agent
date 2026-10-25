//! The agent run loop: one turn at a time — fold the model stream into a
//! transcript, execute tool rounds, fail closed on budgets and
//! stationarity, and emit one run event per stream-json line.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use craft_core::events::TurnCaps;
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, Usage};
use craft_tools::{Registry, ToolCtx};

use crate::budget::{budget_status, sum_usage, zero_usage};
use crate::call::{Flow, STATIONARY_DETAIL, abandon_calls, execute_calls};
use crate::events::{CancelCategory, RuntimeEvent};
use crate::guard::Stationarity;
use crate::outcome::{ExitStatus, RunOutcome};
use crate::prepare::prepare;
use crate::request;
use crate::retry;
use crate::step::{StepCtx, StepStop};

/// Typed run knobs. Defaults mirror the digest mission: 40 turns, 4M
/// total tokens. Env and CLI wiring stay in the app layer, except the
/// step bound and the result window, which resolve their own knobs
/// when unset.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub system: Option<String>,
    /// Model turns allowed before the run fails closed (exit 2).
    pub max_turns: u32,
    /// Total token spend allowed across the run (exit 3); None is
    /// unlimited.
    pub max_total_tokens: Option<u64>,
    /// Per-request output cap forwarded to the provider.
    pub max_tokens: u32,
    /// Total attempts per model step after a mid-stream death; None
    /// resolves `CRAFT_AGENT_STEP_RETRIES` (default 3).
    pub step_attempts: Option<u32>,
    /// Wall-clock budget a transient step failure may consume past the
    /// attempt bound; None resolves `CRAFT_AGENT_RETRY_PATIENCE_SECS`
    /// (default 30 minutes, 0 disables).
    pub step_patience: Option<Duration>,
    /// Turns of tool results kept verbatim in the assembled prompt;
    /// None resolves `CRAFT_AGENT_RESULT_WINDOW_TURNS` (default 0,
    /// trimming off).
    pub result_window_turns: Option<u32>,
    /// Per-result byte cap on the assembled prompt; None resolves
    /// CRAFT_AGENT_RESULT_TRUNCATE_BYTES (default 4096, 0 disables).
    pub result_truncate_bytes: Option<u64>,
    /// Spill write root for truncated tails; None resolves RUNNER_TEMP,
    /// falling back to the platform temp dir.
    pub spill_root: Option<PathBuf>,
    pub turn_caps: TurnCaps,
    /// Outer bound on a stalled event stream; None disables it.
    pub idle_timeout: Option<Duration>,
    pub workspace_root: PathBuf,
    /// Read-denied path globs for this run's own step isolation.
    pub deny_read: Vec<String>,
    /// Write-denied path globs for this run's own step isolation.
    pub deny_write: Vec<String>,
    /// Append-only JSONL transcript; None keeps the run in memory.
    pub session_path: Option<PathBuf>,
    /// The subagent hub this run spawns through; the root wiring
    /// publishes spend and drains child events, children carry a
    /// wiring only so their registries can hold the spawn pair.
    pub subagents: Option<crate::subagent::SubagentWiring>,
    /// Secret redaction applied to tool output, tool errors, and run
    /// detail before any sink; environment-built by default so child
    /// runs self-configure.
    pub redactor: Arc<craft_redact::Redactor>,
}

impl RunConfig {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            system: None,
            max_turns: 40,
            max_total_tokens: Some(4_000_000),
            max_tokens: DEFAULT_REQUEST_TOKENS,
            step_attempts: None,
            step_patience: None,
            result_window_turns: None,
            result_truncate_bytes: None,
            spill_root: None,
            turn_caps: TurnCaps::default(),
            idle_timeout: None,
            workspace_root,
            deny_read: Vec::new(),
            deny_write: Vec::new(),
            session_path: None,
            subagents: None,
            redactor: Arc::new(craft_redact::Redactor::from_env()),
        }
    }
}

/// Per-request output cap when a config leaves it unset.
pub const DEFAULT_REQUEST_TOKENS: u32 = 8192;

/// Drives a mounted provider and a tool registry through the agent loop.
pub struct Runner<P> {
    provider: Arc<P>,
    registry: Registry,
    config: RunConfig,
}

impl<P: Provider + Send + Sync> Runner<P> {
    pub fn new(provider: Arc<P>, registry: Registry, config: RunConfig) -> Self {
        Self {
            provider,
            registry,
            config,
        }
    }

    /// Runs the loop to its terminal outcome; every ending passes
    /// through `finish`, so the terminal event and the outcome never
    /// disagree, after the detached pool drains.
    pub async fn run(&self, prompt: &str, mut sink: impl FnMut(RuntimeEvent) + Send) -> RunOutcome {
        let step_attempts = match self.config.step_attempts {
            Some(attempts) => attempts,
            None => match retry::step_attempts_from_env() {
                Ok(attempts) => attempts,
                Err(err) => return self.finish_error(&mut sink, err.to_string()).await,
            },
        };
        let step_patience = match self.config.step_patience {
            Some(patience) => patience,
            None => match retry::step_patience_from_env() {
                Ok(patience) => patience,
                Err(err) => {
                    return self.finish_error(&mut sink, err.to_string()).await;
                }
            },
        };
        let mut prepared = match prepare(&self.config, prompt).await {
            Ok(prepared) => prepared,
            Err(detail) => return self.finish_error(&mut sink, detail).await,
        };
        let ctx = ToolCtx::with_scopes(
            self.config.workspace_root.clone(),
            craft_tools::scope::ScopeDenies::new(
                self.config.deny_read.clone(),
                self.config.deny_write.clone(),
            ),
        );
        let mut total = zero_usage();
        let mut guard = Stationarity::default();
        let mut turn: u32 = 0;
        loop {
            turn += 1;
            if turn > self.config.max_turns {
                let detail = format!("run aborted after max turns ({})", self.config.max_turns);
                return self
                    .finish(
                        &mut sink,
                        ExitStatus::MaxTurns,
                        CancelCategory::MaxTurns,
                        detail,
                        total,
                        turn - 1,
                    )
                    .await;
            }
            sink(RuntimeEvent::TurnStarted { turn });
            let folded = match self
                .step_ctx()
                .drive(
                    request::build(&self.config, &self.registry, &mut prepared).await,
                    turn,
                    &mut sink,
                    &mut total,
                    step_attempts,
                    step_patience,
                )
                .await
            {
                Ok(folded) => folded,
                Err(StepStop::Provider(err)) => {
                    return self
                        .finish(
                            &mut sink,
                            ExitStatus::Error,
                            CancelCategory::Error,
                            err.to_string(),
                            total,
                            turn - 1,
                        )
                        .await;
                }
                Err(StepStop::Budget(detail)) => {
                    return self
                        .finish(
                            &mut sink,
                            ExitStatus::Budget,
                            CancelCategory::Budget,
                            detail,
                            total,
                            turn - 1,
                        )
                        .await;
                }
            };
            total = sum_usage(total, folded.usage);
            sink(RuntimeEvent::TurnUsage {
                turn,
                usage: folded.usage,
            });
            if let Some(wiring) = &self.config.subagents {
                wiring.publish_parent_turn(total);
            }
            if let Err(err) = prepared
                .session
                .append(ChatMessage::Assistant {
                    text: folded.text.clone(),
                    tool_calls: folded.tool_calls.clone(),
                    native_blocks: folded.native_blocks.clone(),
                })
                .await
            {
                return self
                    .finish(
                        &mut sink,
                        ExitStatus::Error,
                        CancelCategory::Error,
                        err.to_string(),
                        total,
                        turn,
                    )
                    .await;
            }
            if let Some(detail) = budget_status(&total, self.config.max_total_tokens) {
                let (exit, category, reason) =
                    match abandon_calls(&mut prepared.session, &folded.tool_calls, &detail).await {
                        Ok(()) => (ExitStatus::Budget, CancelCategory::Budget, detail),
                        Err(err) => (ExitStatus::Error, CancelCategory::Error, err.to_string()),
                    };
                return self
                    .finish(&mut sink, exit, category, reason, total, turn)
                    .await;
            }
            if folded.tool_calls.is_empty() {
                let detail = String::from("run completed");
                return self
                    .finish(
                        &mut sink,
                        ExitStatus::Ok,
                        CancelCategory::Completed,
                        detail,
                        total,
                        turn,
                    )
                    .await;
            }
            let flow = execute_calls(
                &self.registry,
                &ctx,
                &self.config.redactor,
                &mut prepared.session,
                &folded.tool_calls,
                turn,
                &mut sink,
                &mut guard,
            )
            .await;
            if let Some(wiring) = &self.config.subagents {
                total = sum_usage(total, wiring.after_tool_batch(turn, &mut sink));
                if let Some(detail) = budget_status(&total, self.config.max_total_tokens) {
                    return self
                        .finish(
                            &mut sink,
                            ExitStatus::Budget,
                            CancelCategory::Budget,
                            detail,
                            total,
                            turn,
                        )
                        .await;
                }
            }
            match flow {
                Ok(Flow::Continue) => {}
                Ok(Flow::Stationary) => {
                    return self
                        .finish(
                            &mut sink,
                            ExitStatus::Stationarity,
                            CancelCategory::Stationarity,
                            String::from(STATIONARY_DETAIL),
                            total,
                            turn,
                        )
                        .await;
                }
                Err(err) => {
                    return self
                        .finish(
                            &mut sink,
                            ExitStatus::Error,
                            CancelCategory::Error,
                            err.to_string(),
                            total,
                            turn,
                        )
                        .await;
                }
            }
        }
    }

    /// The per-step driver over the mounted provider and the fold knobs.
    fn step_ctx(&self) -> StepCtx<'_, P> {
        StepCtx {
            provider: self.provider.as_ref(),
            idle_timeout: self.config.idle_timeout,
            turn_caps: self.config.turn_caps,
            max_total_tokens: self.config.max_total_tokens,
        }
    }

    /// The shared endgame lives in `finish`; every ending rides it.
    async fn finish(
        &self,
        sink: &mut (dyn FnMut(RuntimeEvent) + Send),
        exit: ExitStatus,
        category: CancelCategory,
        detail: String,
        usage: Usage,
        turns: u32,
    ) -> RunOutcome {
        crate::finish::finish(&self.config, sink, exit, category, detail, usage, turns).await
    }

    /// The knob-resolution ending: a hostile retry env value is a typed
    /// error before the run starts.
    async fn finish_error(
        &self,
        sink: &mut (dyn FnMut(RuntimeEvent) + Send),
        detail: String,
    ) -> RunOutcome {
        self.finish(
            sink,
            ExitStatus::Error,
            CancelCategory::Error,
            detail,
            zero_usage(),
            0,
        )
        .await
    }
}
