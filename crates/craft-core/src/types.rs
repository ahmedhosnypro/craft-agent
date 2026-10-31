//! Neutral request, message, tool, usage, and stop-reason vocabulary.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// One turn of conversation. Roles are neutral; adapters map to dialect
/// blocks and the runtime appends tool results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ChatMessage {
    User {
        text: String,
        /// Image attachments; absent serializes identically to the
        /// image-less shape.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImagePart>,
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
        /// Image attachments; absent serializes identically to the
        /// image-less shape.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImagePart>,
    },
}

/// An image attachment on a user or tool message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImagePart {
    Base64 { media_type: String, data: String },
    Url { url: String },
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

/// Marker key for tool arguments that arrived malformed from the
/// model; the raw fragment rides as the value, capped.
pub const INVALID_TOOL_ARGUMENTS_KEY: &str = "invalid_tool_arguments";
/// Cap on the raw fragment embedded in the marker.
pub const SALVAGED_ARGUMENTS_MAX_BYTES: usize = 256;

/// Wraps malformed tool arguments into a valid JSON object so every
/// downstream consumer (tool execution, transcript, wire re-encode)
/// stays well-formed. The raw is truncated at a char boundary under
/// the cap.
pub fn salvage_arguments(raw: &str) -> String {
    let mut cut = raw.len().min(SALVAGED_ARGUMENTS_MAX_BYTES);
    while !raw.is_char_boundary(cut) {
        cut -= 1;
    }
    serde_json::json!({ INVALID_TOOL_ARGUMENTS_KEY: &raw[..cut] }).to_string()
}

/// A tool offered to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the parameters object.
    pub parameters: serde_json::Value,
}

/// How the model may pick tools for a turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    Any,
    Tool { name: String },
}

/// Sampling knobs; absent fields mean "provider default".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sampling {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub stop_sequences: Vec<String>,
}

/// A thinking request; providers map it onto their own wire form.
/// The budget rides only when the caller sets it, and the thoughts
/// surface only when asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thinking {
    /// The token budget the model may spend thinking.
    pub budget_tokens: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_thoughts: bool,
}

/// One model turn request; the provider adds model and endpoint config.
/// Messages ride behind `Arc` so the runtime assembles each request by
/// sharing the transcript's entries instead of deep-cloning them; the
/// wire shape is a plain message vector either way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub system: Option<String>,
    #[serde(with = "messages_serde")]
    pub messages: Vec<Arc<ChatMessage>>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Sampling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    /// Thinking level as an arbitrary string; each dialect encoder maps
    /// it (`none`/`off` disables thinking, known levels map onto the
    /// provider's effort/level field, anything else passes through
    /// verbatim).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

/// `none`/`off` disable thinking, case-insensitive.
pub fn thinking_level_disabled(level: &str) -> bool {
    let lower = level.to_ascii_lowercase();
    lower == "none" || lower == "off"
}

/// The named levels the dialect encoders recognize; anything else is a
/// verbatim passthrough.
pub fn known_thinking_level(level: &str) -> bool {
    matches!(
        level.to_ascii_lowercase().as_str(),
        "none" | "off" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}

/// `Arc`-shared messages serialize as the plain message array they
/// hold; deserialization shares nothing yet shares everything new.
mod messages_serde {
    use super::ChatMessage;
    use serde::ser::SerializeSeq;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;

    pub(super) fn serialize<S: Serializer>(
        messages: &[Arc<ChatMessage>],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(messages.len()))?;
        for message in messages {
            seq.serialize_element(message.as_ref())?;
        }
        seq.end()
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Arc<ChatMessage>>, D::Error> {
        Ok(Vec::<ChatMessage>::deserialize(deserializer)?
            .into_iter()
            .map(Arc::new)
            .collect())
    }
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
