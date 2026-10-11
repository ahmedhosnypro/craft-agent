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

use super::chunk::{WireChunk, WireFunctionCall, WirePart};

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
    tool_index: u64,
}

impl Lane {
    fn new(reader: Reader) -> Self {
        Self {
            reader,
            terminal: None,
            usage: None,
            pending: VecDeque::new(),
            ended: false,
            tool_index: 0,
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

/// One SSE event: the data JSON decodes permissively, usageMetadata
/// merges, a blocked prompt is the terminal stop, and the first
/// candidate's parts map onto delta events. The request never asks for
/// more than one candidate, so later ones are ignored.
fn consume_sse(event: SseEvent, lane: &mut Lane) -> Result<bool, ProviderError> {
    let chunk: WireChunk = match serde_json::from_str(&event.data) {
        Ok(chunk) => chunk,
        Err(err) => return chunk_anomaly(lane, format!("invalid chunk JSON: {err}")),
    };
    if let Some(usage) = chunk.usage_metadata {
        let usage = Usage::from(usage);
        lane.usage = Some(lane.usage.map_or(usage, |known| known.merge(usage)));
    }
    if let Some(reason) = chunk
        .prompt_feedback
        .and_then(|feedback| feedback.block_reason)
    {
        record_terminal(lane, StopReason::ContentFilter, &reason)?;
    }
    if let Some(candidate) = chunk.candidates.into_iter().next() {
        if let Some(content) = candidate.content {
            for part in content.parts {
                push_part(lane, part)?;
            }
        }
        if let Some(reason) = candidate.finish_reason {
            record_terminal(lane, StopReason::from_gemini(&reason), &reason)?;
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

/// One candidate part: thought parts and bare signatures never reach
/// the turn, text streams, and a function call lands whole on the next
/// adapter-assigned index.
fn push_part(lane: &mut Lane, part: WirePart) -> Result<(), ProviderError> {
    if let Some(call) = part.function_call {
        return push_tool_call(lane, call);
    }
    let Some(text) = part.text.filter(|text| !text.is_empty()) else {
        return Ok(());
    };
    if part.thought {
        return Ok(());
    }
    if lane.terminal.is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("content delta arrived after finishReason"),
        });
    }
    lane.pending.push_back(ModelEvent::TextDelta { text });
    Ok(())
}

fn push_tool_call(lane: &mut Lane, call: WireFunctionCall) -> Result<(), ProviderError> {
    if lane.terminal.is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("functionCall part arrived after finishReason"),
        });
    }
    let arguments = serde_json::to_string(&call.args.unwrap_or_else(|| serde_json::json!({})))
        .map_err(|err| ProviderError::Transport {
            detail: format!("function args did not re-serialize: {err}"),
        })?;
    lane.pending.push_back(ModelEvent::ToolCallDelta {
        index: lane.tool_index,
        id: call.id,
        name: call.name,
        arguments,
    });
    lane.tool_index = lane.tool_index.saturating_add(1);
    Ok(())
}

fn record_terminal(
    lane: &mut Lane,
    stop_reason: StopReason,
    raw: &str,
) -> Result<(), ProviderError> {
    match &lane.terminal {
        None => lane.terminal = Some(stop_reason),
        Some(known) if *known == stop_reason => {}
        Some(_) => {
            return Err(ProviderError::Transport {
                detail: format!("conflicting finishReason: {raw}"),
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
