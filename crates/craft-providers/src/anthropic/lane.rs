//! The Messages decode lane: named SSE events over one response fold
//! into model events, blocks register at their wire index, the terminal
//! buffers while usage frames merge, and exactly one TurnFinished leaves
//! the lane when the wire finishes.

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, boxed};
use craft_core::sse::{SseError, SseEvent, SseOptions, SseReader};
use craft_core::types::{StopReason, Usage};
use futures::{Stream, StreamExt, stream};

use super::chunk::{
    WireBlockDelta, WireBlockStart, WireBlockStop, WireErrorEnvelope, WireMessageDelta,
    WireMessageStart,
};
use super::map_error_envelope;
use crate::transport;

type Reader = Pin<Box<dyn Stream<Item = Result<SseEvent, SseError<reqwest::Error>>> + Send>>;

/// Mounts the decode lane over one connected response.
pub(super) fn lane_stream(response: reqwest::Response, sse: SseOptions) -> ModelEventStream {
    let reader: Reader = Box::pin(SseReader::new(response.bytes_stream(), sse));
    boxed(stream::unfold(Lane::new(reader), step))
}

/// The block kind registered at one open wire index; a start registers
/// it, a stop clears it, and deltas must match it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Text,
    Tool,
    Other,
}

/// One pass over the wire: open blocks, buffered terminal, merged usage,
/// pending deltas, and the ended flag that closes the stream.
struct Lane {
    reader: Reader,
    blocks: BTreeMap<u64, Block>,
    terminal: Option<StopReason>,
    usage: Option<Usage>,
    pending: VecDeque<ModelEvent>,
    ended: bool,
}

impl Lane {
    fn new(reader: Reader) -> Self {
        Self {
            reader,
            blocks: BTreeMap::new(),
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

/// One SSE event, dispatched by its documented name; ping and unknown
/// names are skipped so a newer protocol never breaks the lane.
fn consume_sse(event: SseEvent, lane: &mut Lane) -> Result<bool, ProviderError> {
    match event.event.as_str() {
        "message_stop" => Ok(true),
        "error" => error_event(&event.data),
        "message_start" => message_start(&event.data, lane),
        "content_block_start" => block_start(&event.data, lane),
        "content_block_delta" => block_delta(&event.data, lane),
        "content_block_stop" => block_stop(&event.data, lane),
        "message_delta" => message_delta(&event.data, lane),
        _ => Ok(false),
    }
}

fn message_start(data: &str, lane: &mut Lane) -> Result<bool, ProviderError> {
    let frame: WireMessageStart = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return anomaly(lane, format!("invalid event JSON: {err}")),
    };
    if let Some(usage) = frame.message.and_then(|message| message.usage) {
        merge_usage(lane, usage.into());
    }
    Ok(false)
}

/// A mid-stream error event surfaces typed; one without an error object
/// is a malformed frame.
fn error_event(data: &str) -> Result<bool, ProviderError> {
    match serde_json::from_str::<WireErrorEnvelope>(data) {
        Ok(envelope) => match envelope.error {
            Some(error) => Err(map_error_envelope(error)),
            None => Err(ProviderError::Transport {
                detail: String::from("error event without an error object"),
            }),
        },
        Err(err) => Err(ProviderError::Transport {
            detail: format!("invalid error event JSON: {err}"),
        }),
    }
}

fn block_stop(data: &str, lane: &mut Lane) -> Result<bool, ProviderError> {
    let frame: WireBlockStop = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return anomaly(lane, format!("invalid event JSON: {err}")),
    };
    lane.blocks.remove(&frame.index);
    Ok(false)
}

fn message_delta(data: &str, lane: &mut Lane) -> Result<bool, ProviderError> {
    let frame: WireMessageDelta = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return anomaly(lane, format!("invalid event JSON: {err}")),
    };
    if let Some(usage) = frame.usage {
        merge_usage(lane, usage.into());
    }
    if let Some(reason) = frame.delta.and_then(|delta| delta.stop_reason) {
        record_terminal(lane, reason)?;
    }
    Ok(false)
}

/// A block header registers the index: tool_use heads emit the call's
/// id and name at once, text and other block kinds register silently.
fn block_start(data: &str, lane: &mut Lane) -> Result<bool, ProviderError> {
    let frame: WireBlockStart = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return anomaly(lane, format!("invalid event JSON: {err}")),
    };
    let block = match frame.content_block {
        Some(block) => block,
        None => return anomaly(lane, String::from("content_block_start without a block")),
    };
    let kind = block.get("type").and_then(serde_json::Value::as_str);
    if kind == Some("tool_use") {
        lane.blocks.insert(frame.index, Block::Tool);
        lane.pending.push_back(ModelEvent::ToolCallDelta {
            index: frame.index,
            id: block
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(String::from),
            name: block
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(String::from),
            arguments: String::new(),
        });
    } else {
        let registered = if kind == Some("text") {
            Block::Text
        } else {
            Block::Other
        };
        lane.blocks.insert(frame.index, registered);
    }
    Ok(false)
}

/// One delta lands on its registered block: text surfaces, tool input
/// fragments through, thinking and future kinds never surface.
fn block_delta(data: &str, lane: &mut Lane) -> Result<bool, ProviderError> {
    let frame: WireBlockDelta = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return anomaly(lane, format!("invalid event JSON: {err}")),
    };
    let delta = match frame.delta {
        Some(delta) => delta,
        None => return anomaly(lane, String::from("content_block_delta without a delta")),
    };
    match delta.get("type").and_then(serde_json::Value::as_str) {
        Some("text_delta") => push_text(lane, frame.index, &delta)?,
        Some("input_json_delta") => push_input(lane, frame.index, &delta)?,
        _ => {}
    }
    Ok(false)
}

fn push_text(lane: &mut Lane, index: u64, delta: &serde_json::Value) -> Result<(), ProviderError> {
    if lane.terminal.is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("content delta arrived after the terminal"),
        });
    }
    let Some(text) = delta.get("text").and_then(serde_json::Value::as_str) else {
        return Err(ProviderError::Transport {
            detail: String::from("text delta without text"),
        });
    };
    match lane.blocks.get(&index) {
        Some(Block::Tool) => Err(ProviderError::Transport {
            detail: format!("text delta on a tool_use block at index {index}"),
        }),
        Some(_) => {
            lane.pending.push_back(ModelEvent::TextDelta {
                text: String::from(text),
            });
            Ok(())
        }
        None => Err(ProviderError::Transport {
            detail: format!("content_block_delta without a content_block_start at index {index}"),
        }),
    }
}

fn push_input(lane: &mut Lane, index: u64, delta: &serde_json::Value) -> Result<(), ProviderError> {
    if lane.terminal.is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("content delta arrived after the terminal"),
        });
    }
    let Some(partial) = delta
        .get("partial_json")
        .and_then(serde_json::Value::as_str)
    else {
        return Err(ProviderError::Transport {
            detail: String::from("input_json_delta without partial_json"),
        });
    };
    match lane.blocks.get(&index) {
        Some(Block::Tool) => {
            lane.pending.push_back(ModelEvent::ToolCallDelta {
                index,
                id: None,
                name: None,
                arguments: String::from(partial),
            });
            Ok(())
        }
        Some(_) => Err(ProviderError::Transport {
            detail: format!("input_json_delta on a non-tool block at index {index}"),
        }),
        None => Err(ProviderError::Transport {
            detail: format!("content_block_delta without a content_block_start at index {index}"),
        }),
    }
}

fn record_terminal(lane: &mut Lane, reason: String) -> Result<(), ProviderError> {
    let stop_reason = StopReason::from_anthropic(&reason);
    match &lane.terminal {
        None => lane.terminal = Some(stop_reason),
        Some(known) if *known == stop_reason => {}
        Some(_) => {
            return Err(ProviderError::Transport {
                detail: format!("conflicting stop_reason: {reason}"),
            });
        }
    }
    Ok(())
}

fn merge_usage(lane: &mut Lane, usage: Usage) {
    lane.usage = Some(lane.usage.map_or(usage, |known| known.merge(usage)));
}

/// A malformed frame after the terminal stands as the recorded turn; one
/// before it is a typed transport failure.
fn anomaly(lane: &Lane, detail: String) -> Result<bool, ProviderError> {
    if lane.terminal.is_some() {
        return Ok(true);
    }
    Err(ProviderError::Transport { detail })
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
