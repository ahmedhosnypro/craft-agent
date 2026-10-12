//! Wire DTOs: permissive by construction — unknown fields are ignored
//! and absent optional fields default, since the surface keeps growing
//! with new optional parameters. Option fields tolerate both a missing
//! key and an explicit null without per-field attributes.

use craft_core::types::Usage;
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct WireChunk {
    pub(super) error: Option<WireError>,
    pub(super) usage: Option<WireUsage>,
    #[serde(default)]
    pub(super) choices: Vec<WireChoice>,
}

#[derive(Deserialize)]
pub(super) struct WireChoice {
    pub(super) delta: Option<WireDelta>,
    pub(super) finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct WireDelta {
    pub(super) content: Option<String>,
    pub(super) tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
pub(super) struct WireToolCall {
    #[serde(default)]
    pub(super) index: u64,
    pub(super) id: Option<String>,
    pub(super) function: Option<WireFunction>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct WireFunction {
    pub(super) name: Option<String>,
    pub(super) arguments: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct WireError {
    pub(super) message: Option<String>,
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    pub(super) code: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct WireUsage {
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) prompt_tokens_details: Option<WireUsageDetails>,
    pub(super) completion_tokens_details: Option<WireUsageDetails>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct WireUsageDetails {
    pub(super) cached_tokens: u64,
    pub(super) reasoning_tokens: u64,
}

impl From<WireUsage> for Usage {
    /// prompt_tokens already includes cached_tokens and completion_tokens
    /// already includes reasoning_tokens; report the disjoint remainders
    /// so the bucket sum matches total_tokens.
    fn from(usage: WireUsage) -> Self {
        let cached = usage
            .prompt_tokens_details
            .map_or(0, |details| details.cached_tokens);
        let reasoning = usage
            .completion_tokens_details
            .map_or(0, |details| details.reasoning_tokens);
        Self {
            input_tokens: usage.prompt_tokens.saturating_sub(cached),
            output_tokens: usage.completion_tokens.saturating_sub(reasoning),
            cache_read_tokens: cached,
            cache_write_tokens: 0,
            reasoning_tokens: reasoning,
            incomplete: false,
        }
    }
}
