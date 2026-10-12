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
//! {"type":"run_finished","exit_code":0,"category":"completed","detail":"...","usage":{...},"turns":2}
//! ```
//!
//! `run_finished` is always the last line; its `exit_code` doubles as the
//! process exit code and `category` is the cancellation category. A
//! `turn_retried` line marks a replayed model step: the partial text the
//! run already streamed for that turn is void, `attempt` is the 1-based
//! number of the failed attempt, and `reason` is a stable token.

use serde::{Deserialize, Serialize};

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
    RunFinished {
        exit_code: i32,
        category: CancelCategory,
        detail: String,
        usage: Usage,
        turns: u32,
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
    SensitivePath,
    EscapesWorkspace,
}
