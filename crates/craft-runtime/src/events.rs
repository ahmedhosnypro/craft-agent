//! The NDJSON run-event contract `craft-agent run --output-format
//! stream-json` serializes: one JSON object per line, tagged by `type`.
//!
//! ```text
//! {"type":"turn_started","turn":1}
//! {"type":"assistant_delta","turn":1,"text":"..."}
//! {"type":"turn_retried","turn":1,"attempt":1,"reason":"stream_closed"}
//! {"type":"tool_call","turn":1,"index":0,"id":"c1","name":"read_file","arguments":"{}"}
//! {"type":"tool_progress","turn":1,"name":"bash","stream":"stdout","text":"..."}
//! {"type":"tool_result","turn":1,"call_id":"c1","name":"read_file","is_error":false,"content":"..."}
//! {"type":"turn_usage","turn":1,"usage":{...Usage...}}
//! {"type":"compacted","turn":1,"attempt":1,"kept_messages":4,"summary_bytes":812}
//! {"type":"run_finished","exit_code":0,"category":"completed","detail":"...","usage":{...},"turns":2}
//! ```
//!
//! `run_finished` is always the last line of a `run`; its `exit_code`
//! doubles as the process exit code and `category` is the cancellation
//! category. A `turn_retried` line marks a replayed model step: the
//! partial text the run already streamed for that turn is void,
//! `attempt` is the 1-based number of the failed attempt, and `reason`
//! is a stable token.
//!
//! The `workflow` mission frames its stream with two additive lines —
//! `workflow_started` (run id, plan name, plan hash, args digest) and
//! `workflow_finished` (exit code plus the serialized workflow report),
//! which is that mission's last line. Steps run inside a workflow do
//! not emit runtime events; their records live in the workflow
//! journal on disk.
//!
//! Subagent spawns add two more additive lines - `subagent_started`
//! and `subagent_finished` - drained right after the tool batch that
//! spawned them, so they sit between that batch's `tool_result` and
//! the next `turn_started`, and `run_finished` stays last. The
//! child's own per-turn events never reach the parent stream.

use serde::{Deserialize, Serialize};

use std::borrow::Cow;

use craft_core::types::Usage;

/// Which child stream a progress chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStreamName {
    Stdout,
    Stderr,
}

/// One emitted run event; exactly one serializes per NDJSON line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    TurnStarted {
        turn: u32,
    },
    AssistantDelta {
        turn: u32,
        text: String,
    },
    TurnRetried {
        turn: u32,
        attempt: u32,
        reason: String,
    },
    ToolCall {
        turn: u32,
        index: u64,
        id: String,
        name: String,
        arguments: String,
    },
    ToolProgress {
        turn: u32,
        name: String,
        stream: ToolStreamName,
        text: String,
    },
    ToolResult {
        turn: u32,
        call_id: String,
        name: String,
        is_error: bool,
        content: String,
    },
    TurnUsage {
        turn: u32,
        usage: Usage,
    },
    /// The run compacted its history: the prefix became one summary
    /// message and the last `kept_messages` entries stayed verbatim.
    /// `attempt` is the 1-based compaction attempt count for the run.
    Compacted {
        turn: u32,
        attempt: u32,
        kept_messages: usize,
        summary_bytes: usize,
    },
    RunFinished {
        exit_code: i32,
        category: CancelCategory,
        detail: String,
        usage: Usage,
        turns: u32,
    },
    /// A workflow run began: the plan identity and the resolved args
    /// digest. Frames the `workflow` mission's stream.
    WorkflowStarted {
        run_id: String,
        name: String,
        plan_hash: String,
        args_digest: String,
    },
    /// A workflow run ended: the process exit code and the serialized
    /// workflow report. The `workflow` mission's last line.
    WorkflowFinished {
        exit_code: i32,
        report: serde_json::Value,
    },
    /// A subagent launched from this turn's tool batch: the hub id,
    /// the profile, and the child's caps. The child's per-turn events
    /// never reach the parent stream — its transcript is the audit
    /// channel.
    SubagentStarted {
        turn: u32,
        id: String,
        profile: String,
        max_turns: u32,
        max_tokens: Option<u64>,
        timeout_secs: Option<u64>,
    },
    /// A subagent settled: its stop reason from the run taxonomy,
    /// its own usage and turns, and its transcript pointer. Drained
    /// after the tool batch, before the next `turn_started`.
    SubagentFinished {
        turn: u32,
        id: String,
        profile: String,
        stop_reason: String,
        detail: String,
        usage: Usage,
        turns: u32,
        session_path: Option<String>,
    },
}

/// Why the run ended; serializes snake_case on the terminal event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelCategory {
    Completed,
    Error,
    MaxTurns,
    Stationarity,
    Budget,
}

/// Redact the string fields an event carries past the runtime
/// boundary. Model deltas stream untouched: the tool seam already
/// redacted their sources, and chunk edges can split needles.
pub fn redact_event(event: &mut RuntimeEvent, redactor: &craft_redact::Redactor) {
    fn scrub(field: &mut String, redactor: &craft_redact::Redactor) {
        if let Cow::Owned(clean) = redactor.redact(field) {
            *field = clean;
        }
    }
    match event {
        RuntimeEvent::ToolCall { arguments, .. } => scrub(arguments, redactor),
        RuntimeEvent::ToolProgress { text, .. } => scrub(text, redactor),
        RuntimeEvent::ToolResult { content, .. } => scrub(content, redactor),
        RuntimeEvent::TurnRetried { reason, .. } => scrub(reason, redactor),
        RuntimeEvent::RunFinished { detail, .. } => scrub(detail, redactor),
        RuntimeEvent::SubagentFinished { detail, .. } => scrub(detail, redactor),
        _ => {}
    }
}
