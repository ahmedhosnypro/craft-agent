//! The agent run loop: one turn at a time — fold the model stream into a
//! transcript, execute tool rounds, fail closed on budgets, stationarity,
//! and denials, and emit one run event per stream-json line.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use craft_core::error::ProviderError;
use craft_core::events::{FoldedTurn, ModelEvent, TurnCaps, TurnFold};
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest, ToolSpec, Usage};
use craft_tools::{Registry, ToolCtx};
use futures::StreamExt;

use crate::budget::{budget_status, fold_error, sum_usage, zero_usage};
use crate::call::{Flow, execute_calls};
use crate::events::{CancelCategory, RuntimeEvent};
use crate::guard::Stationarity;
use crate::outcome::{ExitStatus, RunOutcome};
use crate::session::{Session, SessionError};

/// Typed run knobs. Defaults mirror the digest mission: 40 turns, 4M
/// total tokens. Env and CLI wiring stay in the app layer.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub system: Option<String>,
    /// Model turns allowed before the run fails closed (exit 2).
    pub max_turns: u32,
    /// Total token spend allowed across the run (exit 3).
    pub max_total_tokens: u64,
    /// Per-request output cap forwarded to the provider.
    pub max_tokens: u32,
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
        let mut session = match self.open_session() {
            Ok(session) => session,
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
        };
        if let Err(err) = session.append(ChatMessage::User {
            text: String::from(prompt),
        }) {
            return self.finish(
                &mut sink,
                ExitStatus::Error,
                CancelCategory::Error,
                err.to_string(),
                zero_usage(),
                0,
            );
        }
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
            let folded = match self.drive_turn(&session, turn, &mut sink).await {
                Ok(folded) => folded,
                Err(err) => {
                    return self.finish(
                        &mut sink,
                        ExitStatus::Error,
                        CancelCategory::Error,
                        err.to_string(),
                        total,
                        turn - 1,
                    );
                }
            };
            if let Err(err) = folded.require_content() {
                return self.finish(
                    &mut sink,
                    ExitStatus::Error,
                    CancelCategory::Error,
                    err.to_string(),
                    total,
                    turn,
                );
            }
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
            if let Some(detail) = budget_status(&total, self.config.max_total_tokens) {
                return self.finish(
                    &mut sink,
                    ExitStatus::Budget,
                    CancelCategory::Budget,
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
                    let detail = String::from(
                        "stationary run stopped: the same tool call repeated 8 times in a row",
                    );
                    return self.finish(
                        &mut sink,
                        ExitStatus::Stationarity,
                        CancelCategory::Stationarity,
                        detail,
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

    /// Streams one model turn to a folded turn, forwarding text deltas as
    /// events and bounding a stalled stream by the idle timeout.
    async fn drive_turn(
        &self,
        session: &Session,
        turn: u32,
        sink: &mut dyn FnMut(RuntimeEvent),
    ) -> Result<FoldedTurn, ProviderError> {
        let request = self.build_request(session);
        let mut stream = self.provider.stream(request).await?;
        let mut fold = TurnFold::new(self.config.turn_caps);
        loop {
            let next = match self.config.idle_timeout {
                Some(limit) => tokio::time::timeout(limit, stream.next())
                    .await
                    .map_err(|_| ProviderError::IdleTimeout)?,
                None => stream.next().await,
            };
            let Some(item) = next else {
                break;
            };
            let event = item?;
            if let ModelEvent::TextDelta { text } = &event {
                sink(RuntimeEvent::AssistantDelta {
                    turn,
                    text: text.clone(),
                });
            }
            fold.push(event).map_err(fold_error)?;
        }
        fold.finish().map_err(fold_error)
    }

    /// The next request: the full model-visible transcript plus the
    /// registry's cached tool specs, so replayed native blocks ride
    /// their assistant messages verbatim.
    fn build_request(&self, session: &Session) -> ModelRequest {
        let tools = self
            .registry
            .names()
            .iter()
            .filter_map(|name| self.registry.spec(name))
            .map(|spec| ToolSpec {
                name: spec.name.clone(),
                description: spec.description.clone(),
                parameters: spec.schema.clone(),
            })
            .collect();
        ModelRequest {
            system: self.config.system.clone(),
            messages: session.messages().to_vec(),
            tools,
            max_tokens: self.config.max_tokens,
        }
    }

    fn open_session(&self) -> Result<Session, SessionError> {
        match &self.config.session_path {
            Some(path) => Session::open(path.clone()),
            None => Ok(Session::in_memory()),
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
