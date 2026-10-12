//! The Messages decode lane: named SSE events dispatch onto the block
//! registry, the terminal buffers while usage frames merge, and exactly
//! one TurnFinished leaves the lane when the wire finishes.
//! Provider-native thinking blocks accumulate for replay as native events.

use std::collections::BTreeMap;

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::ModelEventStream;
use craft_core::sse::{SseEvent, SseOptions};
use craft_core::types::{NativeDialect, StopReason};

use super::chunk::{
    NativeAccum, NativeEmit, WireBlockDelta, WireBlockStart, WireBlockStop, WireErrorEnvelope,
    WireMessageDelta, WireMessageStart,
};
use super::map_error_envelope;
use crate::lane_driver::{LaneCommon, LaneDialect, mount};

/// Bound on blocks open at once; a hostile run of never-stopped starts
/// is a typed failure, not unbounded growth.
const MAX_OPEN_BLOCKS: usize = 256;

/// Bound on thinking and signature text held per open native block.
const MAX_NATIVE_ACCUM_BYTES: usize = 1024 * 1024;

/// Mounts the decode lane over one connected response.
pub(super) fn lane_stream(response: reqwest::Response, sse: SseOptions) -> ModelEventStream {
    mount(response, sse, Registry::default())
}

/// The block kind registered at one open wire index; a start registers
/// it, a stop clears it, and deltas must match it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Text,
    Tool,
    Native,
    Other,
}

/// The Messages decode state: the block registry at the wire indices.
#[derive(Debug, Default)]
pub(super) struct Registry {
    blocks: BTreeMap<u64, Block>,
    natives: BTreeMap<u64, NativeAccum>,
}

impl LaneDialect for Registry {
    /// One SSE event, dispatched by its documented name; ping and unknown
    /// names are skipped so a newer protocol never breaks the lane.
    fn consume(&mut self, common: &mut LaneCommon, event: SseEvent) -> Result<bool, ProviderError> {
        match event.event.as_str() {
            "message_stop" => Ok(true),
            "error" => error_event(&event.data),
            "message_start" => message_start(&event.data, common),
            "content_block_start" => block_start(&event.data, self, common),
            "content_block_delta" => block_delta(&event.data, self, common),
            "content_block_stop" => block_stop(&event.data, self, common),
            "message_delta" => message_delta(&event.data, common),
            _ => Ok(false),
        }
    }
}

fn message_start(data: &str, common: &mut LaneCommon) -> Result<bool, ProviderError> {
    let frame: WireMessageStart = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return common.anomaly(format!("invalid event JSON: {err}")),
    };
    if let Some(usage) = frame.message.and_then(|message| message.usage) {
        common.record_usage(usage.into());
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

/// A stop closes its block; a native block emits its reconstructed wire
/// shape as one native event, an empty thinking block is a legal skip,
/// and a redacted block without data is a typed failure.
fn block_stop(
    data: &str,
    registry: &mut Registry,
    common: &mut LaneCommon,
) -> Result<bool, ProviderError> {
    let frame: WireBlockStop = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return common.anomaly(format!("invalid event JSON: {err}")),
    };
    let native = registry.natives.remove(&frame.index);
    registry.blocks.remove(&frame.index);
    let Some(accum) = native else {
        return Ok(false);
    };
    match accum.emit() {
        NativeEmit::Block(block) => common.push_pending(ModelEvent::NativeBlock {
            dialect: NativeDialect::Anthropic,
            block,
        }),
        NativeEmit::Anomaly(detail) => {
            return common.anomaly(format!("{detail} at index {}", frame.index));
        }
        NativeEmit::Skip => {}
    }
    Ok(false)
}

fn message_delta(data: &str, common: &mut LaneCommon) -> Result<bool, ProviderError> {
    let frame: WireMessageDelta = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return common.anomaly(format!("invalid event JSON: {err}")),
    };
    if let Some(usage) = frame.usage {
        common.record_usage(usage.into());
    }
    if let Some(reason) = frame.delta.and_then(|delta| delta.stop_reason) {
        record_terminal(common, reason)?;
    }
    Ok(false)
}

/// A block header registers the index: tool_use heads emit the call's
/// id and name at once, native heads open an accumulator, and the rest
/// register silently.
fn block_start(
    data: &str,
    registry: &mut Registry,
    common: &mut LaneCommon,
) -> Result<bool, ProviderError> {
    let frame: WireBlockStart = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return common.anomaly(format!("invalid event JSON: {err}")),
    };
    let block = match frame.content_block {
        Some(block) => block,
        None => return common.anomaly(String::from("content_block_start without a block")),
    };
    let kind = block.get("type").and_then(serde_json::Value::as_str);
    if !registry.blocks.contains_key(&frame.index) && registry.blocks.len() >= MAX_OPEN_BLOCKS {
        return common.anomaly(format!("open content blocks exceeded {MAX_OPEN_BLOCKS}"));
    }
    match kind {
        Some("tool_use") => {
            registry.blocks.insert(frame.index, Block::Tool);
            common.push_pending(ModelEvent::ToolCallDelta {
                index: frame.index,
                id: wire_field(&block, "id"),
                name: wire_field(&block, "name"),
                arguments: String::new(),
            });
        }
        Some(kind @ ("thinking" | "redacted_thinking")) => {
            registry.blocks.insert(frame.index, Block::Native);
            registry
                .natives
                .insert(frame.index, NativeAccum::from_header(kind, &block));
        }
        Some("text") => {
            registry.blocks.insert(frame.index, Block::Text);
        }
        _ => {
            registry.blocks.insert(frame.index, Block::Other);
        }
    }
    Ok(false)
}

/// One string field of a block header, as an owned string.
fn wire_field(block: &serde_json::Value, key: &str) -> Option<String> {
    Some(block.get(key)?.as_str()?.to_owned())
}

/// One delta lands on its registered block: text surfaces, tool input
/// fragments through, native thinking text accumulates for replay.
fn block_delta(
    data: &str,
    registry: &mut Registry,
    common: &mut LaneCommon,
) -> Result<bool, ProviderError> {
    let frame: WireBlockDelta = match serde_json::from_str(data) {
        Ok(frame) => frame,
        Err(err) => return common.anomaly(format!("invalid event JSON: {err}")),
    };
    let delta = match frame.delta {
        Some(delta) => delta,
        None => return common.anomaly(String::from("content_block_delta without a delta")),
    };
    match delta.get("type").and_then(serde_json::Value::as_str) {
        Some("text_delta") => push_text(common, registry, frame.index, &delta)?,
        Some("input_json_delta") => push_input(common, registry, frame.index, &delta)?,
        Some(kind @ ("thinking_delta" | "signature_delta")) => {
            return push_native_delta(common, registry, frame.index, &delta, kind);
        }
        _ => {}
    }
    Ok(false)
}

/// One thinking or signature delta accumulates on its native block
/// under the per-block cap; it never surfaces as text.
fn push_native_delta(
    common: &mut LaneCommon,
    registry: &mut Registry,
    index: u64,
    delta: &serde_json::Value,
    kind: &str,
) -> Result<bool, ProviderError> {
    ensure_pre_terminal(common)?;
    let field = kind.strip_suffix("_delta").unwrap_or("thinking");
    let Some(fragment) = delta.get(field).and_then(serde_json::Value::as_str) else {
        return Err(ProviderError::Transport {
            detail: format!("{kind} without its {field} string"),
        });
    };
    let Some(accum) = registry.natives.get_mut(&index) else {
        return Err(ProviderError::Transport {
            detail: format!("{kind} on a non-native block at index {index}"),
        });
    };
    let NativeAccum::Thinking {
        thinking,
        signature,
    } = accum
    else {
        return Err(ProviderError::Transport {
            detail: format!("{kind} on a redacted block at index {index}"),
        });
    };
    let held = thinking
        .len()
        .saturating_add(signature.len())
        .saturating_add(fragment.len());
    if held > MAX_NATIVE_ACCUM_BYTES {
        return common.anomaly(format!(
            "native block at index {index} exceeded {MAX_NATIVE_ACCUM_BYTES} bytes"
        ));
    }
    if kind == "signature_delta" {
        signature.push_str(fragment);
    } else {
        thinking.push_str(fragment);
    }
    Ok(false)
}

/// Deltas are content: after the terminal the stream is corrupt.
fn ensure_pre_terminal(common: &LaneCommon) -> Result<(), ProviderError> {
    if common.terminal().is_none() {
        return Ok(());
    }
    Err(ProviderError::Transport {
        detail: String::from("content delta arrived after the terminal"),
    })
}

fn push_text(
    common: &mut LaneCommon,
    registry: &Registry,
    index: u64,
    delta: &serde_json::Value,
) -> Result<(), ProviderError> {
    ensure_pre_terminal(common)?;
    let Some(text) = delta.get("text").and_then(serde_json::Value::as_str) else {
        return Err(ProviderError::Transport {
            detail: String::from("text delta without text"),
        });
    };
    match registry.blocks.get(&index) {
        Some(Block::Tool) => Err(ProviderError::Transport {
            detail: format!("text delta on a tool_use block at index {index}"),
        }),
        Some(_) => {
            common.push_pending(ModelEvent::TextDelta {
                text: String::from(text),
            });
            Ok(())
        }
        None => Err(ProviderError::Transport {
            detail: format!("content_block_delta without a content_block_start at index {index}"),
        }),
    }
}

fn push_input(
    common: &mut LaneCommon,
    registry: &Registry,
    index: u64,
    delta: &serde_json::Value,
) -> Result<(), ProviderError> {
    ensure_pre_terminal(common)?;
    let Some(partial) = delta
        .get("partial_json")
        .and_then(serde_json::Value::as_str)
    else {
        return Err(ProviderError::Transport {
            detail: String::from("input_json_delta without partial_json"),
        });
    };
    match registry.blocks.get(&index) {
        Some(Block::Tool) => {
            common.push_pending(ModelEvent::ToolCallDelta {
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

fn record_terminal(common: &mut LaneCommon, reason: String) -> Result<(), ProviderError> {
    let stop_reason = StopReason::from_anthropic(&reason);
    match common.terminal() {
        None => {
            common.set_terminal(stop_reason);
            Ok(())
        }
        Some(known) if known == stop_reason => Ok(()),
        Some(_) => Err(ProviderError::Transport {
            detail: format!("conflicting stop_reason: {reason}"),
        }),
    }
}
