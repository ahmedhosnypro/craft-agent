//! Wire DTOs: permissive by construction — unknown fields are ignored
//! and absent optional fields default, since the surface keeps growing
//! with new optional parameters.

use craft_core::types::Usage;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireChunk {
    #[serde(default)]
    pub(super) candidates: Vec<WireCandidate>,
    pub(super) prompt_feedback: Option<WirePromptFeedback>,
    pub(super) usage_metadata: Option<WireUsage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WirePromptFeedback {
    pub(super) block_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireCandidate {
    pub(super) content: Option<WireContent>,
    pub(super) finish_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireContent {
    #[serde(default)]
    pub(super) parts: Vec<WirePart>,
}

/// The part union: one data field set at most, plus the thought flags.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WirePart {
    #[serde(default)]
    pub(super) thought: bool,
    pub(super) text: Option<String>,
    pub(super) function_call: Option<WireFunctionCall>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WireFunctionCall {
    pub(super) id: Option<String>,
    pub(super) name: Option<String>,
    pub(super) args: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct WireUsage {
    pub(super) prompt_token_count: u64,
    pub(super) candidates_token_count: u64,
    pub(super) cached_content_token_count: u64,
    pub(super) thoughts_token_count: u64,
    pub(super) total_token_count: u64,
}

impl From<WireUsage> for Usage {
    /// promptTokenCount already includes cachedContentTokenCount; report
    /// the uncached remainder so the buckets stay disjoint.
    fn from(usage: WireUsage) -> Self {
        Self {
            input_tokens: usage
                .prompt_token_count
                .saturating_sub(usage.cached_content_token_count),
            output_tokens: usage.candidates_token_count,
            cache_read_tokens: usage.cached_content_token_count,
            cache_write_tokens: 0,
            reasoning_tokens: usage.thoughts_token_count,
            incomplete: false,
        }
    }
}
