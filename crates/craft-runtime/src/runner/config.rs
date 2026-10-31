//! The run's typed knobs: every limit, cap, and path the loop honors.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use craft_core::events::TurnCaps;

use crate::subagent::SubagentWiring;

/// Typed run knobs. Defaults mirror the digest mission: 40 turns, 4M
/// total tokens. Env and CLI wiring stay in the app layer, except the
/// step bound and the result window, which resolve their own knobs
/// when unset.
#[derive(Debug, Clone)]
pub struct RunConfig {
    pub system: Option<String>,
    /// Model turns allowed before the run fails closed (exit 2).
    pub max_turns: u32,
    /// Total token spend allowed across the run (exit 3); None is
    /// unlimited.
    pub max_total_tokens: Option<u64>,
    /// Per-request output cap forwarded to the provider.
    pub max_tokens: u32,
    /// Context window cap in tokens; enables auto-compaction when set.
    pub max_context_tokens: Option<u64>,
    /// Input-side compaction trigger limit; None falls back to the
    /// context cap.
    pub max_input_tokens: Option<u64>,
    /// Per-request output cap override, folded into `max_tokens` by
    /// the app layer; kept here for the compaction summary cap.
    pub max_output_tokens: Option<u64>,
    /// Thinking level forwarded verbatim to the provider dialect;
    /// `none`/`off` disables thinking.
    pub thinking_level: Option<String>,
    /// Total attempts per model step after a mid-stream death; None
    /// resolves `CRAFT_AGENT_STEP_RETRIES` (default 3).
    pub step_attempts: Option<u32>,
    /// Wall-clock budget a transient step failure may consume past the
    /// attempt bound; None resolves `CRAFT_AGENT_RETRY_PATIENCE_SECS`
    /// (default 30 minutes, 0 disables).
    pub step_patience: Option<Duration>,
    /// Turns of tool results kept verbatim in the assembled prompt;
    /// None resolves `CRAFT_AGENT_RESULT_WINDOW_TURNS` (default 0,
    /// trimming off).
    pub result_window_turns: Option<u32>,
    /// Per-result byte cap on the assembled prompt; None resolves
    /// CRAFT_AGENT_RESULT_TRUNCATE_BYTES (default 4096, 0 disables).
    pub result_truncate_bytes: Option<u64>,
    /// Spill write root for truncated tails; None resolves RUNNER_TEMP,
    /// falling back to the platform temp dir.
    pub spill_root: Option<PathBuf>,
    pub turn_caps: TurnCaps,
    /// Outer bound on a stalled event stream; None disables it.
    pub idle_timeout: Option<Duration>,
    pub workspace_root: PathBuf,
    /// Read-denied path globs for this run's own step isolation.
    pub deny_read: Vec<String>,
    /// Write-denied path globs for this run's own step isolation.
    pub deny_write: Vec<String>,
    /// Append-only JSONL transcript; None keeps the run in memory.
    pub session_path: Option<PathBuf>,
    /// The subagent hub this run spawns through; the root wiring
    /// publishes spend and drains child events, children carry a
    /// wiring only so their registries can hold the spawn pair.
    pub subagents: Option<SubagentWiring>,
    /// Secret redaction applied to tool output, tool errors, and run
    /// detail before any sink; environment-built by default so child
    /// runs self-configure.
    pub redactor: Arc<craft_redact::Redactor>,
}

impl RunConfig {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            system: None,
            max_turns: 40,
            max_total_tokens: Some(4_000_000),
            max_tokens: DEFAULT_REQUEST_TOKENS,
            max_context_tokens: None,
            max_input_tokens: None,
            max_output_tokens: None,
            thinking_level: None,
            step_attempts: None,
            step_patience: None,
            result_window_turns: None,
            result_truncate_bytes: None,
            spill_root: None,
            turn_caps: TurnCaps::default(),
            idle_timeout: None,
            workspace_root,
            deny_read: Vec::new(),
            deny_write: Vec::new(),
            session_path: None,
            subagents: None,
            redactor: Arc::new(craft_redact::Redactor::from_env()),
        }
    }
}

/// Per-request output cap when a config leaves it unset.
pub const DEFAULT_REQUEST_TOKENS: u32 = 8192;
