//! The agent run loop: one turn at a time — fold the model stream into a
//! transcript, execute tool rounds, fail closed on budgets, stationarity,
//! and denials, and emit one run event per stream-json line.

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
use crate::prepare::{Prepared, prepare};
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
    /// Total token spend allowed across the run (exit 3).
    pub max_total_tokens: u64,
    /// Per-request output cap forwarded to the provider.
    pub max_tokens: u32,
    /// Total attempts per model step after a mid-stream death; None
    /// resolves `CRAFT_AGENT_STEP_RETRIES` (default 3).
    pub step_attempts: Option<u32>,
    /// Turns of tool results kept verbatim in the assembled prompt;
    /// None resolves `CRAFT_AGENT_RESULT_WINDOW_TURNS` (default 20,
    /// 0 disables trimming).
    pub result_window_turns: Option<u32>,
    pub turn_caps: TurnCaps,
    /// Outer bound on a stalled event stream; None disables it.
    pub idle_timeout: Option<Duration>,
    pub workspace_root: PathBuf,
    /// Append-only JSONL transcript; None keeps the run in memory.
    pub session_path: Option<PathBuf>,
}

impl RunConfig {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            system: None,
            max_turns: 40,
            max_total_tokens: 4_000_000,
            max_tokens: 8192,
            step_attempts: None,
            result_window_turns: None,
            turn_caps: TurnCaps::default(),
            idle_timeout: None,
            workspace_root,
            session_path: None,
        }
    }
}

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

    /// Runs the loop to its terminal outcome; every ending passes through
    /// `finish`, so the terminal event and the outcome never disagree.
    pub async fn run(&self, prompt: &str, mut sink: impl FnMut(RuntimeEvent)) -> RunOutcome {
        let step_attempts = match self.config.step_attempts {
            Some(attempts) => attempts,
            None => match retry::step_attempts_from_env() {
                Ok(attempts) => attempts,
                Err(err) => {
                    return self.finish(
                        &mut sink,
                        ExitStatus::Error,
                        CancelCategory::Error,
                        err.to_string(),
                        zero_usage(),
                        0,
                    );
                }
            },
        };
        let Prepared {
            result_window,
            mut session,
        } = match prepare(&self.config, prompt) {
            Ok(prepared) => prepared,
            Err(detail) => {
                return self.finish(
                    &mut sink,
                    ExitStatus::Error,
                    CancelCategory::Error,
                    detail,
                    zero_usage(),
                    0,
                );
            }
        };
        let ctx = ToolCtx::new(self.config.workspace_root.clone());
        let mut total = zero_usage();
        let mut guard = Stationarity::default();
        let mut turn: u32 = 0;
        loop {
            turn += 1;
            if turn > self.config.max_turns {
                let detail = format!("run aborted after max turns ({})", self.config.max_turns);
                return self.finish(
                    &mut sink,
                    ExitStatus::MaxTurns,
                    CancelCategory::MaxTurns,
                    detail,
                    total,
                    turn - 1,
                );
            }
            sink(RuntimeEvent::TurnStarted { turn });
            let folded = match self
                .step_ctx()
                .drive(
                    request::build(&self.config, &self.registry, &session, result_window),
                    turn,
                    &mut sink,
                    &mut total,
                    step_attempts,
                )
                .await
            {
                Ok(folded) => folded,
                Err(StepStop::Provider(err)) => {
                    return self.finish(
                        &mut sink,
                        ExitStatus::Error,
                        CancelCategory::Error,
                        err.to_string(),
                        total,
                        turn - 1,
                    );
                }
                Err(StepStop::Budget(detail)) => {
                    return self.finish(
                        &mut sink,
                        ExitStatus::Budget,
                        CancelCategory::Budget,
                        detail,
                        total,
                        turn - 1,
                    );
                }
            };
            total = sum_usage(total, folded.usage);
            sink(RuntimeEvent::TurnUsage {
                turn,
                usage: folded.usage,
            });
            if let Err(err) = session.append(ChatMessage::Assistant {
                text: folded.text.clone(),
                tool_calls: folded.tool_calls.clone(),
                native_blocks: None,
            }) {
                return self.finish(
                    &mut sink,
                    ExitStatus::Error,
                    CancelCategory::Error,
                    err.to_string(),
                    total,
                    turn,
                );
            }
            if let Some(detail) = budget_status(&total, self.config.max_total_tokens) {
                let (exit, category, reason) =
                    match abandon_calls(&mut session, &folded.tool_calls, &detail) {
                        Ok(()) => (ExitStatus::Budget, CancelCategory::Budget, detail),
                        Err(err) => (ExitStatus::Error, CancelCategory::Error, err.to_string()),
                    };
                return self.finish(&mut sink, exit, category, reason, total, turn);
            }
            if folded.tool_calls.is_empty() {
                let detail = String::from("run completed");
                return self.finish(
                    &mut sink,
                    ExitStatus::Ok,
                    CancelCategory::Completed,
                    detail,
                    total,
                    turn,
                );
            }
            match execute_calls(
                &self.registry,
                &ctx,
                &mut session,
                &folded.tool_calls,
                turn,
                &mut sink,
                &mut guard,
            )
            .await
            {
                Ok(Flow::Continue) => {}
                Ok(Flow::Stationary) => {
                    return self.finish(
                        &mut sink,
                        ExitStatus::Stationarity,
                        CancelCategory::Stationarity,
                        String::from(STATIONARY_DETAIL),
                        total,
                        turn,
                    );
                }
                Ok(Flow::Denied(category, detail)) => {
                    return self.finish(
                        &mut sink,
                        ExitStatus::Denied,
                        category,
                        detail,
                        total,
                        turn,
                    );
                }
                Err(err) => {
                    return self.finish(
                        &mut sink,
                        ExitStatus::Error,
                        CancelCategory::Error,
                        err.to_string(),
                        total,
                        turn,
                    );
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

    fn finish(
        &self,
        sink: &mut dyn FnMut(RuntimeEvent),
        exit: ExitStatus,
        category: CancelCategory,
        detail: String,
        usage: Usage,
        turns: u32,
    ) -> RunOutcome {
        sink(RuntimeEvent::RunFinished {
            exit_code: exit.code(),
            category,
            detail: detail.clone(),
            usage,
            turns,
        });
        RunOutcome {
            exit,
            category,
            detail,
            usage,
            turns,
            session_path: self.config.session_path.clone(),
        }
    }
}
