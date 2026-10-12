//! The streaming fold: assembles arrival-ordered events into a complete
//! turn, fail-closed on hostile fragments.

use std::collections::BTreeMap;

use super::{FoldError, FoldedTurn, ModelEvent, TurnCaps};
use crate::types::{NativeBlocks, NativeDialect, StopReason, ToolCall, Usage};

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

/// Hostile-input caps for native content blocks in one turn.
const MAX_NATIVE_BLOCKS: usize = 64;
const MAX_NATIVE_BLOCK_BYTES: usize = 1024 * 1024;

/// Streaming assembler: push events in arrival order, finish once.
#[derive(Debug, Clone)]
pub struct TurnFold {
    caps: TurnCaps,
    text: String,
    calls: BTreeMap<u64, PartialToolCall>,
    native: Vec<(NativeDialect, serde_json::Value)>,
    finished: Option<(StopReason, Usage)>,
}

impl TurnFold {
    pub fn new(caps: TurnCaps) -> Self {
        Self {
            caps,
            text: String::new(),
            calls: BTreeMap::new(),
            native: Vec::new(),
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
            ModelEvent::NativeBlock { dialect, block } => self.push_native_block(dialect, block),
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
        let stop_reason = match stop_reason {
            StopReason::Other(raw) => StopReason::Other(cap_stop_reason(raw)),
            known => known,
        };
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

    fn push_native_block(
        &mut self,
        dialect: NativeDialect,
        block: serde_json::Value,
    ) -> Result<(), FoldError> {
        if self
            .native
            .first()
            .is_some_and(|(first, _)| *first != dialect)
        {
            return Err(FoldError::NativeDialectMismatch);
        }
        if self.native.len() >= MAX_NATIVE_BLOCKS {
            return Err(FoldError::TooManyNativeBlocks {
                cap: MAX_NATIVE_BLOCKS,
            });
        }
        // A serde_json::Value always serializes; fail closed if one ever refuses.
        let len = serde_json::to_string(&block)
            .map(|serialized| serialized.len())
            .unwrap_or(usize::MAX);
        if len > MAX_NATIVE_BLOCK_BYTES {
            return Err(FoldError::NativeBlockTooLarge {
                len,
                cap: MAX_NATIVE_BLOCK_BYTES,
            });
        }
        self.native.push((dialect, block));
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
        let native_blocks = pack_native_blocks(self.native);
        Ok(FoldedTurn {
            text: self.text,
            tool_calls,
            stop_reason,
            usage,
            native_blocks,
        })
    }
}

fn pack_native_blocks(native: Vec<(NativeDialect, serde_json::Value)>) -> Option<NativeBlocks> {
    let dialect = native.first()?.0;
    let blocks = native.into_iter().map(|(_, block)| block).collect();
    Some(NativeBlocks {
        dialect,
        blocks: serde_json::Value::Array(blocks),
    })
}
