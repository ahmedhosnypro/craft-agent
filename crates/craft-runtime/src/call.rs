//! The tool-call bridge: drives the registry's contracted streams into
//! model-visible tool results and run events. Tool errors round-trip as
//! `is_error` results; sensitive-path and workspace-escape denials
//! abort the run with the exit-4 family.

use craft_core::types::{ChatMessage, ToolCall};
use craft_tools::{OutStream, ToolCtx, ToolError, ToolEvent};
use futures::StreamExt;

use crate::events::{CancelCategory, RuntimeEvent, ToolStreamName};
use crate::guard::{Guard, NUDGE_TEXT, Stationarity};
use crate::session::{Session, SessionError};

/// The stationarity stop detail; shared with the runner so the guard's
/// message and the run record never drift apart.
pub(crate) const STATIONARY_DETAIL: &str =
    "stationary run stopped: the same tool call repeated 8 times in a row";

/// How the flow continues after a turn's tool calls.
pub(crate) enum Flow {
    Continue,
    Stationary,
    Denied(CancelCategory, String),
}

/// Appends one error tool result per call that will never execute, so a
/// terminal exit leaves a transcript the provider accepts on resume:
/// every logged tool_use has its tool_result.
pub(crate) fn abandon_calls(
    session: &mut Session,
    calls: &[ToolCall],
    detail: &str,
) -> Result<(), SessionError> {
    for call in calls {
        session.append(ChatMessage::Tool {
            call_id: call.id.clone(),
            content: format!("tool call not completed: {detail}"),
            is_error: true,
        })?;
    }
    Ok(())
}

/// The single result of one tool call: its model-visible content.
enum ToolOutcome {
    Done {
        content: String,
        is_error: bool,
    },
    Denied {
        category: CancelCategory,
        detail: String,
    },
}

/// Executes every call of a turn in order, appending each result to
/// the session, applying the stationarity guard, and injecting the
/// nudge before the flow continues.
pub(crate) async fn execute_calls(
    registry: &craft_tools::Registry,
    ctx: &ToolCtx,
    session: &mut Session,
    calls: &[ToolCall],
    turn: u32,
    sink: &mut dyn FnMut(RuntimeEvent),
    guard: &mut Stationarity,
) -> Result<Flow, SessionError> {
    let mut nudge = false;
    for (index, call) in calls.iter().enumerate() {
        sink(RuntimeEvent::ToolCall {
            turn,
            index: index as u64,
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
        let outcome = execute_one(registry, ctx, call, turn, sink).await;
        let (content, is_error) = match outcome {
            ToolOutcome::Done { content, is_error } => (content, is_error),
            ToolOutcome::Denied { category, detail } => {
                abandon_calls(session, &calls[index..], &detail)?;
                return Ok(Flow::Denied(category, detail));
            }
        };
        session.append(craft_core::types::ChatMessage::Tool {
            call_id: call.id.clone(),
            content: content.clone(),
            is_error,
        })?;
        sink(RuntimeEvent::ToolResult {
            turn,
            call_id: call.id.clone(),
            name: call.name.clone(),
            is_error,
            content,
        });
        match guard.observe(call) {
            Guard::Stop => {
                abandon_calls(session, &calls[index + 1..], STATIONARY_DETAIL)?;
                return Ok(Flow::Stationary);
            }
            Guard::Nudge => nudge = true,
            Guard::Continue => {}
        }
    }
    if nudge {
        session.append(craft_core::types::ChatMessage::User {
            text: String::from(NUDGE_TEXT),
        })?;
    }
    Ok(Flow::Continue)
}

/// Drives one contracted tool stream to its single result: progress is
/// surfaced as it arrives, the first terminal or typed error wins, and
/// sensitive-path or workspace-escape errors are run-fatal.
async fn execute_one(
    registry: &craft_tools::Registry,
    ctx: &ToolCtx,
    call: &ToolCall,
    turn: u32,
    sink: &mut dyn FnMut(RuntimeEvent),
) -> ToolOutcome {
    let args = parse_args(call);
    let mut stream = registry.execute(&call.name, args, ctx);
    while let Some(item) = stream.next().await {
        match item {
            Ok(ToolEvent::Progress(progress)) => sink(RuntimeEvent::ToolProgress {
                turn,
                name: call.name.clone(),
                stream: stream_name(progress.stream),
                text: progress.text,
            }),
            Ok(ToolEvent::Terminal(result)) => {
                return ToolOutcome::Done {
                    content: result.output,
                    is_error: false,
                };
            }
            Err(err) => return denied_or_error(err),
        }
    }
    ToolOutcome::Done {
        content: String::from("tool stream ended without a terminal result"),
        is_error: true,
    }
}

fn parse_args(call: &ToolCall) -> serde_json::Value {
    let trimmed = call.arguments.trim();
    if trimmed.is_empty() {
        return serde_json::Value::Object(serde_json::Map::new());
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| {
        serde_json::Value::String(format!("invalid tool arguments JSON: {}", call.arguments))
    })
}

fn denied_or_error(err: ToolError) -> ToolOutcome {
    match err {
        ToolError::SensitivePath { .. } => ToolOutcome::Denied {
            category: CancelCategory::SensitivePath,
            detail: format!("sensitive path denial: {err}"),
        },
        ToolError::EscapesWorkspace { .. } => ToolOutcome::Denied {
            category: CancelCategory::EscapesWorkspace,
            detail: format!("workspace escape denial: {err}"),
        },
        other => ToolOutcome::Done {
            content: other.to_string(),
            is_error: true,
        },
    }
}

fn stream_name(stream: OutStream) -> ToolStreamName {
    match stream {
        OutStream::Stdout => ToolStreamName::Stdout,
        OutStream::Stderr => ToolStreamName::Stderr,
    }
}
