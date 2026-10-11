//! The decode lane: SSE events over one response fold into model events,
//! the terminal is buffered while accounting frames merge usage, and
//! exactly one TurnFinished leaves the lane when the wire finishes.

use std::collections::VecDeque;
use std::pin::Pin;

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, boxed};
use craft_core::sse::{SseError, SseEvent, SseOptions, SseReader};
use craft_core::types::{StopReason, Usage};
use futures::{Stream, StreamExt, stream};

use super::chunk::{WireChunk, WireToolCall};
use super::map_error_envelope;

const DONE_MARK: &str = "[DONE]";

type Reader = Pin<Box<dyn Stream<Item = Result<SseEvent, SseError<reqwest::Error>>> + Send>>;

/// Mounts the decode lane over one connected response.
pub(super) fn lane_stream(response: reqwest::Response, sse: SseOptions) -> ModelEventStream {
    let reader: Reader = Box::pin(SseReader::new(response.bytes_stream(), sse));
    boxed(stream::unfold(Lane::new(reader), step))
}

/// One pass over the wire: buffered terminal, merged usage, pending
/// deltas, and the ended flag that closes the stream after the flush.
struct Lane {
    reader: Reader,
    terminal: Option<StopReason>,
    usage: Option<Usage>,
    pending: VecDeque<ModelEvent>,
    ended: bool,
}

impl Lane {
    fn new(reader: Reader) -> Self {
        Self {
            reader,
            terminal: None,
            usage: None,
            pending: VecDeque::new(),
            ended: false,
        }
    }
}

async fn step(lane: Lane) -> Option<(Result<ModelEvent, ProviderError>, Lane)> {
    let mut lane = lane;
    loop {
        if lane.ended {
            return None;
        }
        if let Some(event) = lane.pending.pop_front() {
            return Some((Ok(event), lane));
        }
        match lane.reader.next().await {
            None => return end(lane),
            Some(Err(err)) => {
                if lane.terminal.is_none() {
                    lane.ended = true;
                    return Some((Err(from_sse(err)), lane));
                }
                return end(lane);
            }
            Some(Ok(event)) => match consume_sse(event, &mut lane) {
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
fn end(lane: Lane) -> Option<(Result<ModelEvent, ProviderError>, Lane)> {
    let Some(stop_reason) = lane.terminal.clone() else {
        return Some((
            Err(ProviderError::StreamClosed),
            Lane {
                ended: true,
                ..lane
            },
        ));
    };
    let usage = lane.usage.unwrap_or_else(Usage::omitted);
    Some((
        Ok(ModelEvent::TurnFinished { stop_reason, usage }),
        Lane {
            ended: true,
            ..lane
        },
    ))
}

/// One SSE event: the terminator ends the lane, error envelopes surface
/// typed, usage merges, and the first choice maps onto delta events. The
/// request never asks for more than one completion, so later choices
/// are ignored.
fn consume_sse(event: SseEvent, lane: &mut Lane) -> Result<bool, ProviderError> {
    if event.data.trim() == DONE_MARK {
        return Ok(true);
    }
    let chunk: WireChunk = match serde_json::from_str(&event.data) {
        Ok(chunk) => chunk,
        Err(err) => return chunk_anomaly(lane, format!("invalid chunk JSON: {err}")),
    };
    if let Some(error) = chunk.error {
        return Err(map_error_envelope(error));
    }
    if let Some(usage) = chunk.usage {
        let usage = Usage::from(usage);
        lane.usage = Some(lane.usage.map_or(usage, |known| known.merge(usage)));
    }
    if let Some(choice) = chunk.choices.into_iter().next() {
        if let Some(delta) = choice.delta {
            push_content(lane, delta.content)?;
            for fragment in delta.tool_calls {
                push_tool_fragment(lane, fragment)?;
            }
        }
        if let Some(reason) = choice.finish_reason {
            record_terminal(lane, reason)?;
        }
    }
    Ok(false)
}

/// A malformed chunk after the terminal stands as the recorded turn; one
/// before it is a typed transport failure.
fn chunk_anomaly(lane: &Lane, detail: String) -> Result<bool, ProviderError> {
    if lane.terminal.is_some() {
        return Ok(true);
    }
    Err(ProviderError::Transport { detail })
}

fn push_content(lane: &mut Lane, content: Option<String>) -> Result<(), ProviderError> {
    let Some(text) = content.filter(|text| !text.is_empty()) else {
        return Ok(());
    };
    if lane.terminal.is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("content delta arrived after finish_reason"),
        });
    }
    lane.pending.push_back(ModelEvent::TextDelta { text });
    Ok(())
}

fn push_tool_fragment(lane: &mut Lane, fragment: WireToolCall) -> Result<(), ProviderError> {
    let function = fragment.function.unwrap_or_default();
    let name = function.name;
    let arguments = function.arguments.unwrap_or_default();
    if fragment.id.is_none() && name.is_none() && arguments.is_empty() {
        return Ok(());
    }
    if lane.terminal.is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("tool-call delta arrived after finish_reason"),
        });
    }
    lane.pending.push_back(ModelEvent::ToolCallDelta {
        index: fragment.index,
        id: fragment.id,
        name,
        arguments,
    });
    Ok(())
}

fn record_terminal(lane: &mut Lane, reason: String) -> Result<(), ProviderError> {
    let stop_reason = StopReason::from_openai(&reason);
    match &lane.terminal {
        None => lane.terminal = Some(stop_reason),
        Some(known) if *known == stop_reason => {}
        Some(_) => {
            return Err(ProviderError::Transport {
                detail: format!("conflicting finish_reason: {reason}"),
            });
        }
    }
    Ok(())
}

fn from_sse(err: SseError<reqwest::Error>) -> ProviderError {
    match err {
        SseError::Transport(err) => ProviderError::from(err),
        SseError::Malformed { detail } => ProviderError::Transport { detail },
        SseError::EventTooLarge { cap } => ProviderError::Transport {
            detail: format!("SSE event exceeded {cap} bytes"),
        },
        SseError::Idle { .. } => ProviderError::IdleTimeout,
    }
}
