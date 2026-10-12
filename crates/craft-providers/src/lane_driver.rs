//! The buffered-terminal lane driver shared by the decode lanes: one
//! SSE reader fold that buffers the terminal, merges usage, drains
//! pending deltas, and flushes exactly one TurnFinished when the wire
//! ends — each dialect supplies only the per-event decode.

use std::collections::VecDeque;
use std::pin::Pin;

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, boxed};
use craft_core::sse::{SseError, SseEvent, SseOptions, SseReader};
use craft_core::types::{StopReason, Usage};
use futures::{Stream, StreamExt, stream};

use crate::transport;

type Reader = Pin<Box<dyn Stream<Item = Result<SseEvent, SseError<reqwest::Error>>> + Send>>;

/// The state every decode lane shares: pending model events, the
/// buffered terminal, and the merged usage.
#[derive(Debug, Default)]
pub(crate) struct LaneCommon {
    pending: VecDeque<ModelEvent>,
    terminal: Option<StopReason>,
    usage: Option<Usage>,
}

impl LaneCommon {
    pub(crate) fn pop_pending(&mut self) -> Option<ModelEvent> {
        self.pending.pop_front()
    }

    pub(crate) fn push_pending(&mut self, event: ModelEvent) {
        self.pending.push_back(event);
    }

    pub(crate) fn terminal(&self) -> Option<StopReason> {
        self.terminal.clone()
    }

    pub(crate) fn set_terminal(&mut self, stop_reason: StopReason) {
        self.terminal = Some(stop_reason);
    }

    pub(crate) fn record_usage(&mut self, usage: Usage) {
        self.usage = Some(self.usage.map_or(usage, |known| known.merge(usage)));
    }

    /// A malformed frame after the terminal stands as the recorded turn;
    /// one before it is a typed transport failure.
    pub(crate) fn anomaly(&self, detail: String) -> Result<bool, ProviderError> {
        if self.terminal.is_some() {
            return Ok(true);
        }
        Err(ProviderError::Transport { detail })
    }
}

/// The per-dialect decode: one wire event onto the lane state; `Ok(true)`
/// ends the lane.
pub(crate) trait LaneDialect {
    fn consume(&mut self, common: &mut LaneCommon, event: SseEvent) -> Result<bool, ProviderError>;
}

/// One pass over the wire: the shared state, the per-dialect decode, and
/// the ended flag that closes the stream.
struct Lane<D> {
    reader: Reader,
    common: LaneCommon,
    dialect: D,
    ended: bool,
}

/// Mounts the lane driver over one connected response and dialect.
pub(crate) fn mount<D>(response: reqwest::Response, sse: SseOptions, dialect: D) -> ModelEventStream
where
    D: LaneDialect + Send + 'static,
{
    let reader: Reader = Box::pin(SseReader::new(response.bytes_stream(), sse));
    boxed(stream::unfold(
        Lane {
            reader,
            common: LaneCommon::default(),
            dialect,
            ended: false,
        },
        step,
    ))
}

async fn step<D>(lane: Lane<D>) -> Option<(Result<ModelEvent, ProviderError>, Lane<D>)>
where
    D: LaneDialect + Send + 'static,
{
    let mut lane = lane;
    loop {
        if lane.ended {
            return None;
        }
        if let Some(event) = lane.common.pop_pending() {
            return Some((Ok(event), lane));
        }
        match lane.reader.next().await {
            None => return end(lane),
            Some(Err(err)) => {
                if lane.common.terminal().is_none() {
                    lane.ended = true;
                    return Some((Err(from_sse(err)), lane));
                }
                return end(lane);
            }
            Some(Ok(event)) => match lane.dialect.consume(&mut lane.common, event) {
                Ok(ended) if ended => return end(lane),
                Ok(_) => {}
                Err(err) => {
                    lane.ended = true;
                    return Some((Err(err), lane));
                }
            },
        }
    }
}

/// Ends the lane: the buffered stop reason and merged usage flush as the
/// single TurnFinished; a wire end without one surfaces StreamClosed.
fn end<D>(lane: Lane<D>) -> Option<(Result<ModelEvent, ProviderError>, Lane<D>)>
where
    D: LaneDialect + Send + 'static,
{
    let Some(stop_reason) = lane.common.terminal() else {
        return Some((
            Err(ProviderError::StreamClosed),
            Lane {
                ended: true,
                ..lane
            },
        ));
    };
    let usage = lane.common.usage.unwrap_or_else(Usage::omitted);
    Some((
        Ok(ModelEvent::TurnFinished { stop_reason, usage }),
        Lane {
            ended: true,
            ..lane
        },
    ))
}

fn from_sse(err: SseError<reqwest::Error>) -> ProviderError {
    match err {
        SseError::Transport(err) => transport::classify(err),
        SseError::Malformed { detail } => ProviderError::Transport { detail },
        SseError::EventTooLarge { cap } => ProviderError::Transport {
            detail: format!("SSE event exceeded {cap} bytes"),
        },
        SseError::Idle { .. } => ProviderError::IdleTimeout,
    }
}
