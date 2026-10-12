//! Shared doubles for the runtime corpora: a scripted provider over the
//! real `Provider` seam and script tools over the real `Registry`.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, Provider, boxed};
use craft_core::types::{ModelRequest, StopReason, Usage};
use craft_tools::event::ToolEvent;
use craft_tools::schema::Schema;
use craft_tools::{Tool, ToolCtx, ToolError, ToolStream};
use futures::stream::{self, StreamExt};

/// One scripted model turn.
#[derive(Clone)]
pub enum Turn {
    /// Stream these events in order, then end the stream.
    Events(Vec<Result<ModelEvent, ProviderError>>),
    /// `stream()` itself fails before producing any event.
    Fail(ProviderError),
    /// Never yields; for idle-timeout enforcement.
    Stall,
}

/// Scripted provider: plays turns in order, then repeats the last turn
/// forever, recording every request it received.
pub struct FakeProvider {
    turns: Mutex<VecDeque<Turn>>,
    pub received: Mutex<Vec<ModelRequest>>,
}

impl FakeProvider {
    pub fn new(turns: Vec<Turn>) -> Arc<Self> {
        Arc::new(Self {
            turns: Mutex::new(turns.into()),
            received: Mutex::new(Vec::new()),
        })
    }

    pub fn requests(&self) -> Vec<ModelRequest> {
        self.received
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn next_turn(&self) -> Turn {
        let mut turns = self
            .turns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match turns.len() {
            0 => Turn::Events(Vec::new()),
            1 => turns[0].clone(),
            _ => turns
                .pop_front()
                .unwrap_or_else(|| Turn::Events(Vec::new())),
        }
    }
}

impl Provider for FakeProvider {
    async fn stream(&self, request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        self.received
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(request);
        match self.next_turn() {
            Turn::Fail(err) => Err(err),
            Turn::Stall => Ok(boxed(stream::pending())),
            Turn::Events(events) => Ok(boxed(stream::iter(events))),
        }
    }
}

pub fn usage(input: u64, output: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

/// Usage with the cache and reasoning buckets set, for budget rows.
pub fn bucketed_usage(input: u64, output: u64, cache_read: u64, reasoning: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: 0,
        reasoning_tokens: reasoning,
        incomplete: false,
    }
}

/// A turn that streams text and finishes with the given stop reason.
pub fn text_turn(text: &str, stop: StopReason, used: Usage) -> Turn {
    Turn::Events(vec![
        Ok(ModelEvent::TextDelta {
            text: String::from(text),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: stop,
            usage: used,
        }),
    ])
}

/// A turn that requests one tool call and finishes with ToolUse.
pub fn tool_turn(id: &str, name: &str, args: &str, used: Usage) -> Turn {
    Turn::Events(vec![
        Ok(ModelEvent::ToolCallDelta {
            index: 0,
            id: Some(String::from(id)),
            name: Some(String::from(name)),
            arguments: String::from(args),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: used,
        }),
    ])
}

/// A turn with a single nonzero-usage TurnFinished and nothing else.
pub fn usage_only_turn(used: Usage) -> Turn {
    Turn::Events(vec![Ok(ModelEvent::TurnFinished {
        stop_reason: StopReason::EndTurn,
        usage: used,
    })])
}

pub fn failing_turn(detail: &str) -> Turn {
    Turn::Fail(ProviderError::Transport {
        detail: String::from(detail),
    })
}

pub fn stream_error_turn(detail: &str) -> Turn {
    Turn::Events(vec![Err(ProviderError::Transport {
        detail: String::from(detail),
    })])
}

/// A turn whose stream dies mid-turn: some text arrives, then the typed
/// error. The partial content is void once the step replays.
pub fn dying_turn(err: ProviderError) -> Turn {
    Turn::Events(vec![
        Ok(ModelEvent::TextDelta {
            text: String::from("partial"),
        }),
        Err(err),
    ])
}

/// A complete turn with no content and the given terminal stop reason.
pub fn empty_stop_turn(stop: StopReason, used: Usage) -> Turn {
    Turn::Events(vec![Ok(ModelEvent::TurnFinished {
        stop_reason: stop,
        usage: used,
    })])
}

/// A tool with a fixed outcome, registered into a real `Registry`.
struct ScriptTool {
    tool_name: &'static str,
    events: Vec<Result<ToolEvent, ToolError>>,
}

impl Tool for ScriptTool {
    fn name(&self) -> &'static str {
        self.tool_name
    }

    fn description(&self) -> &'static str {
        "scripted corpus tool"
    }

    fn params_schema(&self) -> Schema {
        Schema::object("scripted corpus tool", &[])
    }

    fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> ToolStream {
        stream::iter(self.events.clone()).boxed()
    }
}

/// Registry with one tool that always returns `output`.
pub fn registry_with_ok(name: &'static str, output: &str) -> craft_tools::Registry {
    let mut registry = craft_tools::Registry::new(craft_tools::ToolLimits::default());
    registry.register(Box::new(ScriptTool {
        tool_name: name,
        events: vec![Ok(ToolEvent::Terminal(craft_tools::ToolResult {
            output: String::from(output),
        }))],
    }));
    registry
}

/// Registry with one tool whose stream always yields the typed error.
pub fn registry_with_error(name: &'static str, err: ToolError) -> craft_tools::Registry {
    let mut registry = craft_tools::Registry::new(craft_tools::ToolLimits::default());
    registry.register(Box::new(ScriptTool {
        tool_name: name,
        events: vec![Err(err)],
    }));
    registry
}

/// Registry with one progress-then-terminal tool.
pub fn registry_with_progress(
    name: &'static str,
    chunks: &[&str],
    output: &str,
) -> craft_tools::Registry {
    let mut events: Vec<Result<ToolEvent, ToolError>> = chunks
        .iter()
        .map(|chunk| {
            Ok(ToolEvent::Progress(craft_tools::ToolProgress::stdout(
                String::from(*chunk),
            )))
        })
        .collect();
    events.push(Ok(ToolEvent::Terminal(craft_tools::ToolResult {
        output: String::from(output),
    })));
    let mut registry = craft_tools::Registry::new(craft_tools::ToolLimits::default());
    registry.register(Box::new(ScriptTool {
        tool_name: name,
        events,
    }));
    registry
}

/// The real builtin registry with the production limits.
pub fn builtin_registry() -> craft_tools::Registry {
    craft_tools::Registry::with_builtins(craft_tools::ToolLimits::default())
}

/// A fresh session path under the workspace's ignored target/tmp,
/// unique per call.
pub fn session_path(tag: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    let _ = std::fs::create_dir_all(&dir);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!("session-{tag}-{}-{n}.jsonl", std::process::id()))
}

/// Run to completion, collecting the emitted events alongside the outcome.
pub async fn run_collect<P>(
    runner: &craft_runtime::Runner<P>,
    prompt: &str,
) -> (craft_runtime::RunOutcome, Vec<craft_runtime::RuntimeEvent>)
where
    P: craft_core::provider::Provider + Send + Sync + 'static,
{
    let mut events = Vec::new();
    let outcome = runner.run(prompt, |event| events.push(event)).await;
    (outcome, events)
}
