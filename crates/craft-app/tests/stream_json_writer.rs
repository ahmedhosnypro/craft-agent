//! RED corpus: the stream-json NDJSON writer vs the runtime event schema.

use std::io::Write;

use craft_app::stream_json::write_event;
use craft_app::{CancelCategory, RuntimeEvent, ToolStreamName, Usage};

fn total_usage() -> Usage {
    Usage {
        input_tokens: 4200,
        output_tokens: 1800,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

#[test]
fn every_event_round_trips_through_one_ndjson_line() {
    let events = vec![
        RuntimeEvent::TurnStarted { turn: 1 },
        RuntimeEvent::AssistantDelta {
            turn: 1,
            text: "hello\nworld".to_string(),
        },
        RuntimeEvent::ToolCall {
            turn: 1,
            index: 0,
            id: "c1".to_string(),
            name: "read_file".to_string(),
            arguments: "{}".to_string(),
        },
        RuntimeEvent::ToolProgress {
            turn: 1,
            name: "bash".to_string(),
            stream: ToolStreamName::Stdout,
            text: "out".to_string(),
        },
        RuntimeEvent::ToolResult {
            turn: 1,
            call_id: "c1".to_string(),
            name: "read_file".to_string(),
            is_error: false,
            content: "bytes".to_string(),
        },
        RuntimeEvent::TurnUsage {
            turn: 1,
            usage: total_usage(),
        },
    ];
    let mut out = Vec::new();
    for event in &events {
        write_event(&mut out, event).expect("write succeeds");
    }
    let text = String::from_utf8(out).expect("valid utf-8");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), events.len());
    for (line, event) in lines.iter().zip(&events) {
        let parsed: RuntimeEvent = serde_json::from_str(line).expect("line parses as schema");
        assert_eq!(&parsed, event);
    }
}

#[test]
fn finished_event_carries_exit_category_and_total_usage() {
    let finished = RuntimeEvent::RunFinished {
        exit_code: 3,
        category: CancelCategory::Budget,
        detail: "token budget exhausted".to_string(),
        usage: total_usage(),
        turns: 4,
    };
    let mut out = Vec::new();
    write_event(&mut out, &finished).expect("write succeeds");
    let line = String::from_utf8(out).expect("valid utf-8");
    assert!(line.contains("\"type\":\"run_finished\""));
    assert!(line.contains("\"exit_code\":3"));
    assert!(line.contains("\"category\":\"budget\""));
    assert!(line.contains("\"input_tokens\":4200"));
    assert!(line.contains("\"output_tokens\":1800"));
    let parsed: RuntimeEvent = serde_json::from_str(&line).expect("finished line parses");
    assert_eq!(parsed, finished);
}

#[test]
fn sink_sequence_writes_finished_last() {
    // The run handler feeds every RuntimeEvent through the writer; the
    // terminal RunFinished must be the final emitted line.
    let sequence = vec![
        RuntimeEvent::TurnStarted { turn: 1 },
        RuntimeEvent::AssistantDelta {
            turn: 1,
            text: "hi".to_string(),
        },
        RuntimeEvent::RunFinished {
            exit_code: 0,
            category: CancelCategory::Completed,
            detail: "done".to_string(),
            usage: total_usage(),
            turns: 1,
        },
    ];
    let mut out = Vec::new();
    for event in &sequence {
        write_event(&mut out, event).expect("write succeeds");
    }
    let text = String::from_utf8(out).expect("valid utf-8");
    let last = text.lines().next_back().expect("non-empty");
    let parsed: RuntimeEvent = serde_json::from_str(last).expect("last line parses");
    assert!(matches!(
        parsed,
        RuntimeEvent::RunFinished {
            exit_code: 0,
            category: CancelCategory::Completed,
            turns: 1,
            ..
        }
    ));
}

struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("pipe closed"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::Error::other("pipe closed"))
    }
}

#[test]
fn write_errors_propagate_as_io_results() {
    let result = write_event(&mut Broken, &RuntimeEvent::TurnStarted { turn: 1 });
    let err = result.expect_err("broken sink must surface an error");
    assert_eq!(err.to_string(), "pipe closed");
}

#[test]
fn plain_writer_renders_human_readable_lines() {
    let events = vec![
        RuntimeEvent::TurnStarted { turn: 1 },
        RuntimeEvent::AssistantDelta {
            turn: 1,
            text: "typing".to_string(),
        },
        RuntimeEvent::RunFinished {
            exit_code: 4,
            category: CancelCategory::SensitivePath,
            detail: "sensitive path denial".to_string(),
            usage: total_usage(),
            turns: 2,
        },
    ];
    let mut out = Vec::new();
    for event in &events {
        craft_app::stream_json::write_plain_event(&mut out, event).expect("plain write succeeds");
    }
    let text = String::from_utf8(out).expect("valid utf-8");
    assert!(text.contains("turn 1"));
    assert!(text.contains("typing"));
    assert!(text.contains("exit 4"));
    assert!(text.contains("sensitive_path"));
}

#[test]
fn plain_category_names_mirror_the_wire_schema() {
    for category in [
        CancelCategory::Completed,
        CancelCategory::Error,
        CancelCategory::MaxTurns,
        CancelCategory::Stationarity,
        CancelCategory::Budget,
        CancelCategory::SensitivePath,
        CancelCategory::EscapesWorkspace,
    ] {
        let finished = RuntimeEvent::RunFinished {
            exit_code: 0,
            category,
            detail: "d".to_string(),
            usage: total_usage(),
            turns: 1,
        };
        let mut wire = Vec::new();
        write_event(&mut wire, &finished).expect("wire write succeeds");
        let wire = String::from_utf8(wire).expect("utf-8");
        let needle = wire
            .split("\"category\":\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("category on the wire")
            .to_string();

        let mut plain = Vec::new();
        craft_app::stream_json::write_plain_event(&mut plain, &finished)
            .expect("plain write succeeds");
        let plain = String::from_utf8(plain).expect("utf-8");

        assert!(plain.contains(&format!("category {needle} ")), "{plain}");
    }
}
