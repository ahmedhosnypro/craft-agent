//! The Gemini decode lane: camelCase chunk JSON folds into model
//! events, the terminal buffers while accounting frames merge usage, and
//! exactly one TurnFinished leaves the lane when the wire finishes.

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::ModelEventStream;
use craft_core::sse::{SseEvent, SseOptions};
use craft_core::types::StopReason;

use super::chunk::{WireChunk, WireFunctionCall, WirePart};
use crate::lane_driver::{LaneCommon, LaneDialect, mount};

/// Mounts the decode lane over one connected response.
pub(super) fn lane_stream(response: reqwest::Response, sse: SseOptions) -> ModelEventStream {
    mount(response, sse, Dialect::default())
}

/// The Gemini decode state: the adapter-assigned tool-call index; the
/// lane driver holds the rest.
#[derive(Debug, Default)]
pub(super) struct Dialect {
    tool_index: u64,
}

impl LaneDialect for Dialect {
    /// One SSE event: the data JSON decodes permissively, usageMetadata
    /// merges, a blocked prompt is the terminal stop, and the first
    /// candidate's parts map onto delta events. The request never asks
    /// for more than one candidate, so later ones are ignored.
    fn consume(&mut self, common: &mut LaneCommon, event: SseEvent) -> Result<bool, ProviderError> {
        let chunk: WireChunk = match serde_json::from_str(&event.data) {
            Ok(chunk) => chunk,
            Err(err) => return common.anomaly(format!("invalid chunk JSON: {err}")),
        };
        if let Some(usage) = chunk.usage_metadata {
            common.record_usage(usage.into());
        }
        if let Some(reason) = chunk
            .prompt_feedback
            .and_then(|feedback| feedback.block_reason)
        {
            record_terminal(common, StopReason::ContentFilter, &reason)?;
        }
        if let Some(candidate) = chunk.candidates.into_iter().next() {
            if let Some(content) = candidate.content {
                for part in content.parts {
                    push_part(common, self, part)?;
                }
            }
            if let Some(reason) = candidate.finish_reason {
                record_terminal(common, StopReason::from_gemini(&reason), &reason)?;
            }
        }
        Ok(false)
    }
}

/// One candidate part: thought parts and bare signatures never reach
/// the turn, text streams, and a function call lands whole on the next
/// adapter-assigned index.
fn push_part(
    common: &mut LaneCommon,
    dialect: &mut Dialect,
    part: WirePart,
) -> Result<(), ProviderError> {
    if let Some(call) = part.function_call {
        return push_tool_call(common, dialect, call);
    }
    let Some(text) = part.text.filter(|text| !text.is_empty()) else {
        return Ok(());
    };
    if part.thought {
        return Ok(());
    }
    if common.terminal().is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("content delta arrived after finishReason"),
        });
    }
    common.push_pending(ModelEvent::TextDelta { text });
    Ok(())
}

fn push_tool_call(
    common: &mut LaneCommon,
    dialect: &mut Dialect,
    call: WireFunctionCall,
) -> Result<(), ProviderError> {
    if common.terminal().is_some() {
        return Err(ProviderError::Transport {
            detail: String::from("functionCall part arrived after finishReason"),
        });
    }
    let arguments = serde_json::to_string(&call.args.unwrap_or_else(|| serde_json::json!({})))
        .map_err(|err| ProviderError::Transport {
            detail: format!("function args did not re-serialize: {err}"),
        })?;
    common.push_pending(ModelEvent::ToolCallDelta {
        index: dialect.tool_index,
        id: call.id,
        name: call.name,
        arguments,
    });
    dialect.tool_index = dialect.tool_index.saturating_add(1);
    Ok(())
}

fn record_terminal(
    common: &mut LaneCommon,
    stop_reason: StopReason,
    raw: &str,
) -> Result<(), ProviderError> {
    match common.terminal() {
        None => {
            common.set_terminal(stop_reason);
            Ok(())
        }
        Some(known) if known == stop_reason => Ok(()),
        Some(_) => Err(ProviderError::Transport {
            detail: format!("conflicting finishReason: {raw}"),
        }),
    }
}
