//! Model events and the fold that assembles a streamed turn into a
//! complete one, fail-closed on hostile fragments.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::ProviderError;
use crate::types::{StopReason, ToolCall, Usage};

/// One streamed model event. Tool-call fragments arrive keyed by the
/// provider's wire index; arguments and names concatenate until the turn
/// finishes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelEvent {
    TextDelta {
        text: String,
    },
    ToolCallDelta {
        index: u64,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    TurnFinished {
        stop_reason: StopReason,
        usage: Usage,
    },
}

/// Bounds the fold against hostile input; defaults never touch honest
/// traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCaps {
    pub max_text_bytes: usize,
    pub max_tool_calls: usize,
    pub max_arguments_bytes: usize,
    pub max_id_bytes: usize,
    pub max_name_bytes: usize,
}

impl Default for TurnCaps {
    fn default() -> Self {
        Self {
            max_text_bytes: 2 * 1024 * 1024,
            max_tool_calls: 256,
            max_arguments_bytes: 1024 * 1024,
            max_id_bytes: 1024,
            max_name_bytes: 1024,
        }
    }
}

/// Assembly failures; adapters map these into the provider taxonomy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FoldError {
    #[error("event arrived after TurnFinished")]
    EventAfterFinished,
    #[error("stream ended without TurnFinished")]
    Unfinished,
    #[error("tool-call id conflict at wire index {index}")]
    DuplicateToolIndex { index: u64 },
    #[error("tool call at wire index {index} never received a name")]
    MissingToolName { index: u64 },
    #[error("tool arguments at wire index {index} are not valid JSON")]
    InvalidArguments { index: u64 },
    #[error("{field} would reach {len} bytes, over the {cap}-byte cap")]
    FieldTooLarge {
        field: &'static str,
        len: usize,
        cap: usize,
    },
    #[error("tool-call count exceeded {cap}")]
    TooManyToolCalls { cap: usize },
}

/// Byte cap for an unknown stop-reason string; the cut lands on a char
/// boundary.
const STOP_REASON_MAX_BYTES: usize = 64;

fn cap_stop_reason(raw: String) -> String {
    if raw.len() <= STOP_REASON_MAX_BYTES {
        return raw;
    }
    let mut cut = STOP_REASON_MAX_BYTES;
    while !raw.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut capped = raw;
    capped.truncate(cut);
    capped
}

#[derive(Debug, Clone, Default)]
struct PartialToolCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

/// Streaming assembler: push events in arrival order, finish once.
#[derive(Debug, Clone)]
pub struct TurnFold {
    caps: TurnCaps,
    text: String,
    calls: BTreeMap<u64, PartialToolCall>,
    finished: Option<(StopReason, Usage)>,
}

impl TurnFold {
    pub fn new(caps: TurnCaps) -> Self {
        Self {
            caps,
            text: String::new(),
            calls: BTreeMap::new(),
            finished: None,
        }
    }

    pub fn push(&mut self, event: ModelEvent) -> Result<(), FoldError> {
        if self.finished.is_some() {
            return match event {
                ModelEvent::TurnFinished { stop_reason, usage } => {
                    self.absorb_repeat(stop_reason, usage)
                }
                _ => Err(FoldError::EventAfterFinished),
            };
        }
        match event {
            ModelEvent::TextDelta { text } => self.push_text(text),
            ModelEvent::ToolCallDelta {
                index,
                id,
                name,
                arguments,
            } => self.push_tool_delta(index, id, name, arguments),
            ModelEvent::TurnFinished { stop_reason, usage } => {
                self.push_finished(stop_reason, usage)
            }
        }
    }

    /// A repeated terminal with the same stop reason is an accounting
    /// frame from the usage chunk: its usage merges into the recorded
    /// terminal and it never becomes a second one. A different stop
    /// reason is a corrupt stream.
    fn absorb_repeat(&mut self, stop_reason: StopReason, usage: Usage) -> Result<(), FoldError> {
        let recorded = match self.finished.as_mut() {
            Some(recorded) => recorded,
            None => return self.push_finished(stop_reason, usage),
        };
        if recorded.0 != stop_reason {
            return Err(FoldError::EventAfterFinished);
        }
        if !usage.incomplete {
            recorded.1 = recorded.1.merge(usage);
        }
        Ok(())
    }

    fn push_text(&mut self, fragment: String) -> Result<(), FoldError> {
        let len = self.text.len().saturating_add(fragment.len());
        if len > self.caps.max_text_bytes {
            return Err(FoldError::FieldTooLarge {
                field: "text",
                len,
                cap: self.caps.max_text_bytes,
            });
        }
        self.text.push_str(&fragment);
        Ok(())
    }

    fn push_tool_delta(
        &mut self,
        index: u64,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    ) -> Result<(), FoldError> {
        let entry = match self.calls.get_mut(&index) {
            Some(entry) => entry,
            None => {
                if self.calls.len() >= self.caps.max_tool_calls {
                    return Err(FoldError::TooManyToolCalls {
                        cap: self.caps.max_tool_calls,
                    });
                }
                self.calls.entry(index).or_default()
            }
        };
        if let Some(id) = id.filter(|value| !value.is_empty()) {
            if entry.id.as_ref().is_some_and(|known| known != &id) {
                return Err(FoldError::DuplicateToolIndex { index });
            }
            if id.len() > self.caps.max_id_bytes {
                return Err(FoldError::FieldTooLarge {
                    field: "tool id",
                    len: id.len(),
                    cap: self.caps.max_id_bytes,
                });
            }
            entry.id = Some(id);
        }
        if let Some(name) = name {
            let len = entry.name.len().saturating_add(name.len());
            if len > self.caps.max_name_bytes {
                return Err(FoldError::FieldTooLarge {
                    field: "tool name",
                    len,
                    cap: self.caps.max_name_bytes,
                });
            }
            entry.name.push_str(&name);
        }
        if !arguments.is_empty() {
            let len = entry.arguments.len().saturating_add(arguments.len());
            if len > self.caps.max_arguments_bytes {
                return Err(FoldError::FieldTooLarge {
                    field: "tool arguments",
                    len,
                    cap: self.caps.max_arguments_bytes,
                });
            }
            entry.arguments.push_str(&arguments);
        }
        Ok(())
    }

    fn push_finished(&mut self, stop_reason: StopReason, usage: Usage) -> Result<(), FoldError> {
        let stop_reason = match stop_reason {
            StopReason::Other(raw) => StopReason::Other(cap_stop_reason(raw)),
            known => known,
        };
        self.finished = Some((stop_reason, usage));
        Ok(())
    }

    /// Completes the turn; every call must have a name and, unless the
    /// call takes no parameters, JSON-parseable arguments.
    pub fn finish(self) -> Result<FoldedTurn, FoldError> {
        let (stop_reason, usage) = self.finished.ok_or(FoldError::Unfinished)?;
        let mut tool_calls = Vec::with_capacity(self.calls.len());
        for (index, partial) in self.calls {
            if partial.name.is_empty() {
                return Err(FoldError::MissingToolName { index });
            }
            if !partial.arguments.is_empty()
                && serde_json::from_str::<serde_json::Value>(&partial.arguments).is_err()
            {
                return Err(FoldError::InvalidArguments { index });
            }
            tool_calls.push(ToolCall {
                id: partial.id.unwrap_or_default(),
                name: partial.name,
                arguments: partial.arguments,
            });
        }
        Ok(FoldedTurn {
            text: self.text,
            tool_calls,
            stop_reason,
            usage,
        })
    }
}

/// One complete turn: assembled text, ordered tool calls, stop reason,
/// and usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldedTurn {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

impl FoldedTurn {
    /// True when the turn produced neither text nor tool calls.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tool_calls.is_empty()
    }

    /// A zero-content terminal stop is an error, never success.
    pub fn require_content(&self) -> Result<(), ProviderError> {
        if self.is_empty() {
            Err(ProviderError::EmptyResponse)
        } else {
            Ok(())
        }
    }
}
