//! Wire decode shapes for the named Messages events: usage frames, block
//! headers, error envelopes, and native block reconstruction, with every
//! field optional so a hostile or future-shaped frame never panics the
//! decode.

use craft_core::types::Usage;
use serde::Deserialize;
use serde_json::Value;

/// The inner error object of an error event: {type, message}.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct WireError {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub message: Option<String>,
}

/// An error event's data: the envelope holding one inner error object.
#[derive(Debug, Deserialize)]
pub(super) struct WireErrorEnvelope {
    pub error: Option<WireError>,
}

/// The usage object every frame carries when it carries one.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub(super) struct WireUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
}

impl From<WireUsage> for Usage {
    /// A frame with no token field at all reports nothing; the turn
    /// stays fail-closed until a real count arrives.
    fn from(usage: WireUsage) -> Self {
        let reported = usage.input_tokens.is_some()
            || usage.output_tokens.is_some()
            || usage.cache_read_input_tokens.is_some()
            || usage.cache_creation_input_tokens.is_some();
        Self {
            input_tokens: usage.input_tokens.unwrap_or(0),
            output_tokens: usage.output_tokens.unwrap_or(0),
            cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
            cache_write_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
            reasoning_tokens: 0,
            incomplete: !reported,
        }
    }
}

/// message_start: the opening frame, carrying the turn's input usage.
#[derive(Debug, Deserialize)]
pub(super) struct WireMessageStart {
    pub message: Option<WireMessageFrame>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WireMessageFrame {
    pub usage: Option<WireUsage>,
}

/// content_block_start: the block header at one wire index. The block
/// object stays raw; the lane reads only the fields it knows.
#[derive(Debug, Deserialize)]
pub(super) struct WireBlockStart {
    pub index: u64,
    pub content_block: Option<Value>,
}

/// content_block_delta: one delta on one wire index.
#[derive(Debug, Deserialize)]
pub(super) struct WireBlockDelta {
    pub index: u64,
    pub delta: Option<Value>,
}

/// content_block_stop: closes the block at one wire index.
#[derive(Debug, Deserialize)]
pub(super) struct WireBlockStop {
    pub index: u64,
}

/// The native accumulator at one open wire index: thinking text and
/// signature append from their deltas, a redacted block carries only
/// the data of its start frame.
#[derive(Debug)]
pub(super) enum NativeAccum {
    Thinking { thinking: String, signature: String },
    Redacted { data: String },
}

/// The outcome of closing one native block.
#[derive(Debug)]
pub(super) enum NativeEmit {
    Block(Value),
    Skip,
    Anomaly(String),
}

impl NativeAccum {
    /// A block header's starting accumulator; only thinking and
    /// redacted_thinking heads open one.
    pub(super) fn from_header(kind: &str, block: &Value) -> Self {
        if kind == "redacted_thinking" {
            return Self::Redacted {
                data: block
                    .get("data")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            };
        }
        Self::Thinking {
            thinking: String::new(),
            signature: String::new(),
        }
    }

    /// The reconstructed wire block at the stop: the native event's
    /// payload, a legal skip for an empty block, or the anomaly detail
    /// for a redacted block without data.
    pub(super) fn emit(self) -> NativeEmit {
        match self {
            Self::Thinking {
                thinking,
                signature,
            } => {
                if thinking.is_empty() && signature.is_empty() {
                    return NativeEmit::Skip;
                }
                let block = if signature.is_empty() {
                    serde_json::json!({"type": "thinking", "thinking": thinking})
                } else {
                    serde_json::json!({
                        "type": "thinking",
                        "thinking": thinking,
                        "signature": signature,
                    })
                };
                NativeEmit::Block(block)
            }
            Self::Redacted { data } => {
                if data.is_empty() {
                    return NativeEmit::Anomaly(String::from("redacted block without data"));
                }
                NativeEmit::Block(serde_json::json!({
                    "type": "redacted_thinking",
                    "data": data,
                }))
            }
        }
    }
}

/// message_delta: the terminal frame — stop reason and cumulative usage.
#[derive(Debug, Deserialize)]
pub(super) struct WireMessageDelta {
    pub delta: Option<WireDelta>,
    pub usage: Option<WireUsage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WireDelta {
    pub stop_reason: Option<String>,
}
