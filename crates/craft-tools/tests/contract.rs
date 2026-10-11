//! Tool-trait semantics: any number of Progress events then exactly one
//! Terminal; a second terminal, a terminal-less stream, or progress after
//! the terminal is a typed error. Registry caches specs.

mod common;

use common::{assert_clean, ctx_for, error, progress, run, terminal, tmp_ws};
use craft_tools::event::ToolEvent;
use craft_tools::schema::Schema;
use craft_tools::tool::{Tool, ToolCtx};
use craft_tools::{Registry, ToolError, ToolLimits, ToolProgress, ToolResult, ToolStream};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A test tool that replays a scripted event list, counting schema builds.
struct Scripted {
    events: Vec<ToolEvent>,
    schema_calls: Arc<AtomicUsize>,
}

impl Scripted {
    fn new(events: Vec<ToolEvent>, schema_calls: Arc<AtomicUsize>) -> Self {
        Self {
            events,
            schema_calls,
        }
    }
}

impl Tool for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }
    fn description(&self) -> &'static str {
        "scripted corpus tool"
    }
    fn params_schema(&self) -> Schema {
        self.schema_calls.fetch_add(1, Ordering::SeqCst);
        Schema::object("scripted args", &[])
    }
    fn execute(&self, _args: Value, _ctx: &ToolCtx) -> ToolStream {
        let items: Vec<Result<ToolEvent, ToolError>> =
            self.events.iter().cloned().map(Ok).collect();
        Box::pin(futures::stream::iter(items))
    }
}

fn p(text: &str) -> ToolEvent {
    ToolEvent::Progress(ToolProgress::stdout(text.to_string()))
}

fn t(text: &str) -> ToolEvent {
    ToolEvent::Terminal(ToolResult {
        output: text.to_string(),
    })
}

async fn scripted(events: Vec<ToolEvent>) -> Vec<Result<ToolEvent, ToolError>> {
    let ws = tmp_ws("contract");
    let mut reg = Registry::new(ToolLimits::default());
    reg.register(Box::new(Scripted::new(
        events,
        Arc::new(AtomicUsize::new(0)),
    )));
    run(reg.execute("scripted", json!({}), &ctx_for(&ws))).await
}

#[tokio::test]
async fn progress_then_terminal_in_order() {
    let events = scripted(vec![p("one"), p("two"), t("done")]).await;
    assert_clean(&events);
    assert_eq!(progress(&events), vec!["one", "two"]);
    assert_eq!(terminal(&events), "done");
}

#[tokio::test]
async fn zero_progress_is_legal() {
    let events = scripted(vec![t("solo")]).await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "solo");
}

#[tokio::test]
async fn second_terminal_is_a_typed_error() {
    let events = scripted(vec![t("first"), t("second")]).await;
    assert!(matches!(error(&events), ToolError::DoubleTerminal));
}

#[tokio::test]
async fn stream_without_terminal_is_a_typed_error() {
    let events = scripted(vec![]).await;
    assert!(matches!(error(&events), ToolError::MissingTerminal));
}

#[tokio::test]
async fn progress_only_stream_is_a_typed_error() {
    let events = scripted(vec![p("one")]).await;
    assert!(matches!(error(&events), ToolError::MissingTerminal));
}

#[tokio::test]
async fn progress_after_terminal_is_a_typed_error() {
    let events = scripted(vec![t("done"), p("late")]).await;
    assert!(matches!(error(&events), ToolError::TrailingProgress));
}

#[tokio::test]
async fn unknown_tool_is_a_typed_error() {
    let ws = tmp_ws("contract-unknown");
    let reg = Registry::new(ToolLimits::default());
    let events = run(reg.execute("nope", json!({}), &ctx_for(&ws))).await;
    assert!(matches!(error(&events), ToolError::UnknownTool { .. }));
}

#[tokio::test]
async fn registry_caches_schemas_and_names_builtins() {
    let ws = tmp_ws("contract-registry");
    let mut reg = Registry::new(ToolLimits::default());
    let calls = Arc::new(AtomicUsize::new(0));
    reg.register(Box::new(Scripted::new(vec![t("x")], calls.clone())));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let _ = reg.spec("scripted");
    let _ = reg.spec("scripted");
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let builtins = Registry::with_builtins(ToolLimits::default());
    let _ = &ws;
    let mut names = builtins.names().to_vec();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "bash",
            "edit_file",
            "gh",
            "glob",
            "grep",
            "read_file",
            "write_file"
        ]
    );
    for name in &names {
        assert!(builtins.spec(name).is_some(), "missing spec for {name}");
    }
}

#[tokio::test]
async fn builtin_spec_shape_is_json_schema() {
    let builtins = Registry::with_builtins(ToolLimits::default());
    let spec = builtins.spec("bash").expect("bash spec");
    assert_eq!(spec.name, "bash");
    assert!(!spec.description.is_empty());
    assert_eq!(spec.schema["type"], "object");
    assert_eq!(spec.schema["properties"]["command"]["type"], "string");
    assert!(
        spec.schema["required"]
            .as_array()
            .is_some_and(|r| r.iter().any(|v| v == "command"))
    );
    assert_eq!(spec.schema["additionalProperties"], false);
}
