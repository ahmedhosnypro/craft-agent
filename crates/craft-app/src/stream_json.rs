//! The headless writers: stream-json (one schema event per NDJSON line)
//! and the plain-text fallback.

use std::io::{self, Write};

use craft_runtime::{CancelCategory, RuntimeEvent};

/// stream-json: one JSON object per line, per the runtime event schema.
pub fn write_event(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let mut line = serde_json::to_string(event).map_err(io::Error::other)?;
    line.push('\n');
    out.write_all(line.as_bytes())
}

fn category_name(category: CancelCategory) -> &'static str {
    match category {
        CancelCategory::Completed => "completed",
        CancelCategory::Error => "error",
        CancelCategory::MaxTurns => "max_turns",
        CancelCategory::Stationarity => "stationarity",
        CancelCategory::Budget => "budget",
        CancelCategory::SensitivePath => "sensitive_path",
        CancelCategory::EscapesWorkspace => "escapes_workspace",
    }
}

/// plain: the same events as human-readable lines.
pub fn write_plain_event(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    match event {
        RuntimeEvent::TurnStarted { turn } => writeln!(out, "== turn {turn} =="),
        RuntimeEvent::AssistantDelta { text, .. } => {
            out.write_all(text.as_bytes()).and_then(|_| writeln!(out))
        }
        RuntimeEvent::TurnRetried {
            turn,
            attempt,
            reason,
        } => writeln!(out, "[retry turn {turn}] attempt {attempt} died: {reason}"),
        RuntimeEvent::ToolCall { name, .. } => writeln!(out, "[tool] {name}"),
        RuntimeEvent::ToolProgress {
            name, text, stream, ..
        } => writeln!(out, "[{name} {stream:?}] {text}"),
        RuntimeEvent::ToolResult {
            name,
            is_error,
            content,
            ..
        } => {
            if *is_error {
                writeln!(out, "[{name} error] {content}")
            } else {
                writeln!(out, "[{name}] {content}")
            }
        }
        RuntimeEvent::TurnUsage { turn, usage } => writeln!(
            out,
            "[usage turn {turn}] in={} out={}",
            usage.input_tokens, usage.output_tokens
        ),
        RuntimeEvent::RunFinished {
            exit_code,
            category,
            detail,
            usage,
            turns,
        } => writeln!(
            out,
            "run finished: exit {exit_code} category {} turns {turns} \
             in={} out={} — {detail}",
            category_name(*category),
            usage.input_tokens,
            usage.output_tokens
        ),
    }
}
