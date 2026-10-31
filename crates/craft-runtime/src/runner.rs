//! The agent run loop: one turn at a time — fold the model stream into a
//! transcript, execute tool rounds, fail closed on budgets and
//! stationarity, and emit one run event per stream-json line.

use std::sync::Arc;

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

mod config;
mod runner_compact;

pub use config::{DEFAULT_REQUEST_TOKENS, RunConfig};

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
        let mut compaction = runner_compact::Compaction::new();
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
            // Auto-compaction runs only when a context cap is set: past
            // the trigger, one summary call swaps the history prefix.
            compaction
                .maybe_compact(
                    self.provider.as_ref(),
                    &self.config,
                    &mut prepared.session,
                    turn,
                    &mut sink,
                )
                .await;
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
            compaction.note_turn(folded.usage.input_tokens, folded.usage.incomplete);
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
