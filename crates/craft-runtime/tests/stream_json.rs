//! Stream-json contract corpus: every emitted event serializes to one
//! NDJSON line that parses back against the documented schema, and the
//! final event carries exit code, category, and total usage.

mod common;

use common::*;
use craft_core::events::ModelEvent;
use craft_core::types::StopReason;
use craft_runtime::RuntimeEvent;
use craft_runtime::{CancelCategory, RunConfig, Runner, ToolStreamName};
use std::path::PathBuf;

fn config() -> RunConfig {
    RunConfig::new(PathBuf::from("."))
}

#[tokio::test]
async fn every_line_parses_and_the_full_vocabulary_appears_in_order() {
    let provider = FakeProvider::new(vec![
        tool_turn("c1", "scripted", "{}", usage(3, 2)),
        Turn::Events(vec![
            Ok(ModelEvent::TextDelta {
                text: String::from("half "),
            }),
            Ok(ModelEvent::TextDelta {
                text: String::from("done"),
            }),
            Ok(ModelEvent::TurnFinished {
                stop_reason: StopReason::EndTurn,
                usage: usage(2, 4),
            }),
        ]),
    ]);
    let runner = Runner::new(
        provider,
        registry_with_progress("scripted", &["chunk-1", "chunk-2"], "out"),
        config(),
    );
    let (outcome, events) = run_collect(&runner, "go").await;
    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(events.len(), 11);

    // Every event is exactly one parseable NDJSON line that round-trips.
    let tags: Vec<String> = events
        .iter()
        .map(|event| {
            let line = serde_json::to_string(event).unwrap();
            let parsed: RuntimeEvent = serde_json::from_str(&line).unwrap();
            assert_eq!(parsed, event.clone());
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            String::from(value["type"].as_str().unwrap_or(""))
        })
        .collect();

    let expected: Vec<String> = [
        "turn_started",
        "turn_usage",
        "tool_call",
        "tool_progress",
        "tool_progress",
        "tool_result",
        "turn_started",
        "assistant_delta",
        "assistant_delta",
        "turn_usage",
        "run_finished",
    ]
    .iter()
    .map(|tag| String::from(*tag))
    .collect();
    assert_eq!(tags, expected);

    let finished = match events.last() {
        Some(RuntimeEvent::RunFinished {
            exit_code,
            category,
            usage,
            turns,
            detail,
        }) => (*exit_code, *category, *usage, *turns, detail.clone()),
        _ => panic!("run must end with run_finished"),
    };
    assert_eq!(finished.0, 0);
    assert_eq!(finished.1, CancelCategory::Completed);
    assert_eq!(finished.2.input_tokens, 5);
    assert_eq!(finished.2.output_tokens, 6);
    assert_eq!(finished.3, 2);
    assert!(finished.4.contains("completed"));

    // Progress payloads carry their stream and text verbatim.
    assert!(events.iter().any(|e| matches!(
        e,
        RuntimeEvent::ToolProgress { text, stream, .. }
        if text == "chunk-2" && *stream == ToolStreamName::Stdout
    )));
}
