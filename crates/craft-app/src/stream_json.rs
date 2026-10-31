//! The headless writers: stream-json (one schema event per NDJSON line)
//! and the plain-text fallback.

use std::io::{self, Write};

use craft_runtime::{CancelCategory, ExitStatus, RuntimeEvent};

use crate::Usage;
use crate::cli::OutputFormat;

/// stream-json: one JSON object per line, per the runtime event schema.
pub fn write_event(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let mut line = serde_json::to_string(event).map_err(io::Error::other)?;
    line.push('\n');
    out.write_all(line.as_bytes())
}

/// The terminal `run_finished` event straight to `out`: the last stream
/// line for a run that ends before the event queue exists. A write
/// failure only costs a stderr note; the typed error and the process
/// exit code still carry the failure.
pub fn write_terminal_line(mut out: impl Write, format: OutputFormat, detail: String) {
    let usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    };
    let event = RuntimeEvent::RunFinished {
        exit_code: ExitStatus::Error.code(),
        category: CancelCategory::Error,
        detail,
        usage,
        turns: 0,
    };
    let result = match format {
        OutputFormat::StreamJson => write_event(&mut out, &event),
        OutputFormat::Plain => write_plain_event(&mut out, &event),
    };
    if let Err(error) = result {
        eprintln!("craft-agent: terminal run-event write failed: {error}");
    }
}

fn category_name(category: CancelCategory) -> &'static str {
    match category {
        CancelCategory::Completed => "completed",
        CancelCategory::Error => "error",
        CancelCategory::MaxTurns => "max_turns",
        CancelCategory::Stationarity => "stationarity",
        CancelCategory::Budget => "budget",
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
        RuntimeEvent::ToolResult { .. } => write_tool_result(out, event),
        RuntimeEvent::TurnUsage { .. } => write_turn_usage(out, event),
        RuntimeEvent::Compacted {
            turn,
            attempt,
            kept_messages,
            summary_bytes,
        } => writeln!(
            out,
            "[compact turn {turn}] attempt {attempt}: history prefix summarized ({summary_bytes} bytes), last {kept_messages} messages kept"
        ),
        RuntimeEvent::RunFinished { .. } => write_run_finished(out, event),
        RuntimeEvent::WorkflowStarted { .. } => write_workflow_started(out, event),
        RuntimeEvent::WorkflowFinished { exit_code, report } => {
            writeln!(out, "workflow finished: exit {exit_code} — {report}")
        }
        RuntimeEvent::SubagentStarted { .. } => write_subagent_started(out, event),
        RuntimeEvent::SubagentFinished { .. } => write_subagent_finished(out, event),
    }
}

fn write_tool_result(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let RuntimeEvent::ToolResult {
        name,
        is_error,
        content,
        ..
    } = event
    else {
        return Ok(());
    };
    if *is_error {
        writeln!(out, "[{name} error] {content}")
    } else {
        writeln!(out, "[{name}] {content}")
    }
}

fn write_turn_usage(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let RuntimeEvent::TurnUsage { turn, usage } = event else {
        return Ok(());
    };
    writeln!(
        out,
        "[usage turn {turn}] in={} out={}",
        usage.input_tokens, usage.output_tokens
    )
}

fn write_run_finished(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let RuntimeEvent::RunFinished {
        exit_code,
        category,
        detail,
        usage,
        turns,
    } = event
    else {
        return Ok(());
    };
    writeln!(
        out,
        "run finished: exit {exit_code} category {} turns {turns} \
         in={} out={} — {detail}",
        category_name(*category),
        usage.input_tokens,
        usage.output_tokens
    )
}

fn write_workflow_started(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let RuntimeEvent::WorkflowStarted {
        run_id,
        name,
        plan_hash,
        args_digest,
    } = event
    else {
        return Ok(());
    };
    writeln!(
        out,
        "workflow `{name}` started: run {run_id} plan {plan_hash} args {args_digest}"
    )
}

fn write_subagent_started(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let RuntimeEvent::SubagentStarted {
        turn,
        id,
        profile,
        max_turns,
        max_tokens,
        timeout_secs,
    } = event
    else {
        return Ok(());
    };
    let timeout = match timeout_secs {
        Some(secs) => format!("{secs}s"),
        None => String::from("none"),
    };
    let budget = match max_tokens {
        Some(tokens) => tokens.to_string(),
        None => String::from("unlimited"),
    };
    writeln!(
        out,
        "[subagent turn {turn}] {id} `{profile}` started: max_turns {max_turns} budget {budget} timeout {timeout}"
    )
}

fn write_subagent_finished(out: &mut dyn Write, event: &RuntimeEvent) -> io::Result<()> {
    let RuntimeEvent::SubagentFinished {
        turn,
        id,
        profile,
        stop_reason,
        detail,
        usage,
        turns,
        session_path,
    } = event
    else {
        return Ok(());
    };
    let tokens = usage.input_tokens
        + usage.cache_read_tokens
        + usage.cache_write_tokens
        + usage.output_tokens
        + usage.reasoning_tokens;
    let transcript = session_path.as_deref().unwrap_or("(in memory)");
    writeln!(
        out,
        "[subagent turn {turn}] {id} `{profile}` finished: {stop_reason} turns {turns} tokens {tokens} — {detail} ({transcript})"
    )
}
