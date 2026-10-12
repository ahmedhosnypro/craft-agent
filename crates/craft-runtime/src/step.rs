//! One model step under the step ladder: stream, fold, and — when the
//! stream dies mid-turn — replay the step from the intact transcript.

use std::time::Duration;

use craft_core::error::ProviderError;
use craft_core::events::{FoldedTurn, ModelEvent, TurnCaps, TurnFold};
use craft_core::provider::Provider;
use craft_core::types::{ModelRequest, Usage};
use futures::StreamExt;

use crate::budget::{budget_status, fold_error, sum_usage};
use crate::events::RuntimeEvent;
use crate::retry;

/// Why a step ended the run instead of folding: the provider error
/// itself, or the budget guard refusing to fund another replay.
pub(crate) enum StepStop {
    Provider(ProviderError),
    Budget(String),
}

/// Where one attempt failed. The seam splits connect-phase failures
/// (already through the provider's ladder) from in-stream deaths (the
/// step ladder's to replay); fold failures are semantic.
enum AttemptError {
    Connect(ProviderError),
    Stream(ProviderError),
    Fold(ProviderError),
}

/// The pieces of the runner one step drives on.
pub(crate) struct StepCtx<'a, P> {
    pub(crate) provider: &'a P,
    pub(crate) idle_timeout: Option<Duration>,
    pub(crate) turn_caps: TurnCaps,
    pub(crate) max_total_tokens: u64,
}

impl<P: Provider + Send + Sync> StepCtx<'_, P> {
    /// Streams one model turn to a folded turn under the step ladder: a
    /// stream that dies mid-turn replays the same request, discarding
    /// the partial fold, and an empty-but-complete turn replays unless
    /// the provider filtered it. Every replay first charges its usage
    /// to the budget — a replay the budget cannot fund ends the run
    /// instead of looping.
    pub(crate) async fn drive(
        &self,
        request: ModelRequest,
        turn: u32,
        sink: &mut dyn FnMut(RuntimeEvent),
        total: &mut Usage,
        attempts: u32,
    ) -> Result<FoldedTurn, StepStop> {
        let mut attempt = 0u32;
        loop {
            attempt = attempt.saturating_add(1);
            let (err, replay) = match self.attempt(request.clone(), turn, sink).await {
                Ok(folded) => match folded.require_content() {
                    Ok(()) => return Ok(folded),
                    Err(err) => {
                        *total = sum_usage(*total, folded.usage);
                        (err, retry::empty_replay(&folded.stop_reason, attempt))
                    }
                },
                Err(AttemptError::Stream(err)) => {
                    let replay = retry::stream_replay(&err, attempt);
                    (err, replay)
                }
                Err(AttemptError::Connect(err)) | Err(AttemptError::Fold(err)) => {
                    return Err(StepStop::Provider(err));
                }
            };
            if attempt >= attempts {
                return Err(StepStop::Provider(err));
            }
            let Some((reason, wait)) = replay else {
                return Err(StepStop::Provider(err));
            };
            if let Some(detail) = budget_status(total, self.max_total_tokens) {
                return Err(StepStop::Budget(detail));
            }
            sink(RuntimeEvent::TurnRetried {
                turn,
                attempt,
                reason: String::from(reason),
            });
            tokio::time::sleep(wait).await;
        }
    }
}

impl<P: Provider + Send + Sync> StepCtx<'_, P> {
    /// One streaming attempt. The seam tags where a failure surfaced:
    /// connect-phase errors already climbed the provider ladder, so the
    /// step ladder replays only in-stream deaths; fold failures are
    /// semantic and never replay.
    async fn attempt(
        &self,
        request: ModelRequest,
        turn: u32,
        sink: &mut dyn FnMut(RuntimeEvent),
    ) -> Result<FoldedTurn, AttemptError> {
        let mut stream = self
            .provider
            .stream(request)
            .await
            .map_err(AttemptError::Connect)?;
        let mut fold = TurnFold::new(self.turn_caps);
        loop {
            let next = match self.idle_timeout {
                Some(limit) => tokio::time::timeout(limit, stream.next())
                    .await
                    .map_err(|_| AttemptError::Stream(ProviderError::IdleTimeout))?,
                None => stream.next().await,
            };
            let Some(item) = next else {
                break;
            };
            let event = item.map_err(AttemptError::Stream)?;
            if let ModelEvent::TextDelta { text } = &event {
                sink(RuntimeEvent::AssistantDelta {
                    turn,
                    text: text.clone(),
                });
            }
            fold.push(event)
                .map_err(|err| AttemptError::Fold(fold_error(err)))?;
        }
        fold.finish()
            .map_err(|err| AttemptError::Fold(fold_error(err)))
    }
}
