//! Neutral request, message, tool, usage, and stop-reason vocabulary.

use serde::{Deserialize, Serialize};

/// One turn of conversation. Roles are neutral; adapters map to dialect
/// blocks and the runtime appends tool results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ChatMessage {
    User {
        text: String,
    },
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
        /// Dialect-native blocks kept for replay; adapters re-send them
        /// on the next turn or the provider rejects the request.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_blocks: Option<NativeBlocks>,
    },
    Tool {
        call_id: String,
        content: String,
        is_error: bool,
    },
}

/// Which dialect a passthrough block set belongs to; adapters reject a
/// mismatch instead of re-encoding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeDialect {
    Anthropic,
    Gemini,
}

/// Provider-native assistant blocks (thinking blocks, thought
/// signatures) carried verbatim; dropping them on replay is a protocol
/// error, so they ride the neutral message as-is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeBlocks {
    pub dialect: NativeDialect,
    pub blocks: serde_json::Value,
}

/// A tool call exactly as the model produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned id; empty when the dialect has none.
    pub id: String,
    pub name: String,
    /// Raw JSON text, unmodified; the tool layer parses it.
    pub arguments: String,
}

/// A tool offered to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the parameters object.
    pub parameters: serde_json::Value,
}

/// One model turn request; the provider adds model and endpoint config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
}

/// Why a turn stopped, across every dialect. Unknown finish reasons land
/// in Other and keep the raw provider string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    ContentFilter,
    Refusal,
    PausedTurn,
    MalformedToolCall,
    ContextWindowExceeded,
    Other(String),
}

impl StopReason {
    /// OpenAI Chat Completions finish_reason, case-insensitive; the legacy
    /// function_call form still means the model called a tool.
    pub fn from_openai(reason: &str) -> Self {
        if reason.eq_ignore_ascii_case("stop") {
            Self::EndTurn
        } else if reason.eq_ignore_ascii_case("length") {
            Self::MaxTokens
        } else if reason.eq_ignore_ascii_case("tool_calls")
            || reason.eq_ignore_ascii_case("function_call")
        {
            Self::ToolUse
        } else if reason.eq_ignore_ascii_case("content_filter") {
            Self::ContentFilter
        } else {
            Self::Other(String::from(reason))
        }
    }

    /// Anthropic Messages stop_reason, case-insensitive.
    pub fn from_anthropic(reason: &str) -> Self {
        if reason.eq_ignore_ascii_case("end_turn") {
            Self::EndTurn
        } else if reason.eq_ignore_ascii_case("max_tokens") {
            Self::MaxTokens
        } else if reason.eq_ignore_ascii_case("stop_sequence") {
            Self::StopSequence
        } else if reason.eq_ignore_ascii_case("tool_use") {
            Self::ToolUse
        } else if reason.eq_ignore_ascii_case("pause_turn") {
            Self::PausedTurn
        } else if reason.eq_ignore_ascii_case("refusal") {
            Self::Refusal
        } else if reason.eq_ignore_ascii_case("model_context_window_exceeded") {
            Self::ContextWindowExceeded
        } else {
            Self::Other(String::from(reason))
        }
    }
}

impl StopReason {
    /// Gemini generateContent finishReason; values arrive in any case.
    pub fn from_gemini(reason: &str) -> Self {
        match reason.to_ascii_uppercase().as_str() {
            "STOP" => Self::EndTurn,
            "MAX_TOKENS" => Self::MaxTokens,
            "SAFETY" | "RECITATION" | "LANGUAGE" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => {
                Self::ContentFilter
            }
            "MALFORMED_FUNCTION_CALL" | "UNEXPECTED_TOOL_CALL" => Self::MalformedToolCall,
            _ => Self::Other(String::from(reason)),
        }
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Token usage per turn in disjoint buckets; cache fields serialize only
/// when non-zero. `incomplete` is the fail-closed flag for turns where
/// the provider omitted usage entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_read_tokens: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub incomplete: bool,
}

impl Usage {
    /// Marker for a turn whose usage the provider never reported.
    pub fn omitted() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            incomplete: true,
        }
    }

    /// Per-bucket max: providers report cumulative or once-per-turn totals,
    /// so the larger report wins and repeated reports never double-count.
    /// The incomplete flag ORs, fail-closed.
    pub fn merge(self, later: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.max(later.input_tokens),
            output_tokens: self.output_tokens.max(later.output_tokens),
            cache_read_tokens: self.cache_read_tokens.max(later.cache_read_tokens),
            cache_write_tokens: self.cache_write_tokens.max(later.cache_write_tokens),
            reasoning_tokens: self.reasoning_tokens.max(later.reasoning_tokens),
            incomplete: self.incomplete || later.incomplete,
        }
    }
}
