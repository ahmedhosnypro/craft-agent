//! Model events and the fold that assembles a streamed turn into a
//! complete one, fail-closed on hostile fragments.

mod fold;

pub use fold::TurnFold;

use serde::{Deserialize, Serialize};

use crate::error::ProviderError;
use crate::types::{NativeBlocks, NativeDialect, StopReason, ToolCall, Usage};

/// One streamed model event. Tool-call fragments arrive keyed by the
/// provider's wire index; arguments and names concatenate until the turn
/// finishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// One completed provider-native content block, carried verbatim.
    NativeBlock {
        dialect: NativeDialect,
        block: serde_json::Value,
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
    #[error("native-block count exceeded {cap}")]
    TooManyNativeBlocks { cap: usize },
    #[error("native block would reach {len} bytes, over the {cap}-byte cap")]
    NativeBlockTooLarge { len: usize, cap: usize },
    #[error("native blocks from two different dialects in one turn")]
    NativeDialectMismatch,
}

/// One complete turn: assembled text, ordered tool calls, native blocks,
/// stop reason, and usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldedTurn {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_blocks: Option<NativeBlocks>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

impl FoldedTurn {
    /// True when the turn produced neither text, tool calls, nor native
    /// blocks.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tool_calls.is_empty() && self.native_blocks.is_none()
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
