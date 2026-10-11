//! Fold semantics: text and tool-call fragments accumulate by wire index
//! into a complete turn, and hostile fragments fail closed with typed
//! errors instead of panicking.

use craft_core::error::ProviderError;
use craft_core::events::{FoldError, FoldedTurn, ModelEvent, TurnCaps, TurnFold};
use craft_core::types::{StopReason, ToolCall, Usage};

fn text(s: &str) -> ModelEvent {
    ModelEvent::TextDelta {
        text: String::from(s),
    }
}

fn delta(index: u64, id: Option<&str>, name: Option<&str>, args: &str) -> ModelEvent {
    ModelEvent::ToolCallDelta {
        index,
        id: id.map(String::from),
        name: name.map(String::from),
        arguments: String::from(args),
    }
}

fn end_turn() -> ModelEvent {
    ModelEvent::TurnFinished {
        stop_reason: StopReason::EndTurn,
        usage: Usage::omitted(),
    }
}

fn fold(events: &[ModelEvent]) -> Result<FoldedTurn, FoldError> {
    let mut fold = TurnFold::new(TurnCaps::default());
    for event in events {
        fold.push(event.clone())?;
    }
    fold.finish()
}

#[test]
fn text_fragments_concatenate() -> Result<(), FoldError> {
    let turn = fold(&[text("hel"), text("lo "), text("world"), end_turn()])?;
    assert_eq!(turn.text, "hello world");
    Ok(())
}

#[test]
fn empty_text_fragments_accumulate_nothing() -> Result<(), FoldError> {
    let turn = fold(&[text(""), text("a"), text(""), end_turn()])?;
    assert_eq!(turn.text, "a");
    Ok(())
}

#[test]
fn tool_argument_fragments_concatenate_by_wire_index() -> Result<(), FoldError> {
    let turn = fold(&[
        delta(0, Some("call_1"), Some("bash"), "{\"a\":"),
        delta(0, None, None, "1}"),
        end_turn(),
    ])?;
    assert_eq!(
        turn.tool_calls,
        vec![ToolCall {
            id: String::from("call_1"),
            name: String::from("bash"),
            arguments: String::from("{\"a\":1}"),
        }]
    );
    Ok(())
}

#[test]
fn tool_name_fragments_concatenate() -> Result<(), FoldError> {
    let turn = fold(&[
        delta(0, Some("c1"), Some("ba"), "{}"),
        delta(0, None, Some("sh"), ""),
        end_turn(),
    ])?;
    assert_eq!(turn.tool_calls[0].name, "bash");
    Ok(())
}

#[test]
fn tool_calls_keep_wire_index_order() -> Result<(), FoldError> {
    let turn = fold(&[
        delta(1, Some("c2"), Some("second"), "{}"),
        text("interleaved"),
        delta(0, Some("c1"), Some("first"), "{}"),
        delta(1, None, None, ""),
        end_turn(),
    ])?;
    assert_eq!(turn.tool_calls.len(), 2);
    assert_eq!(turn.tool_calls[0].id, "c1");
    assert_eq!(turn.tool_calls[1].id, "c2");
    assert_eq!(turn.text, "interleaved");
    Ok(())
}

#[test]
fn duplicate_index_with_same_id_continues() -> Result<(), FoldError> {
    let turn = fold(&[
        delta(0, Some("c1"), Some("bash"), "{}"),
        delta(0, Some("c1"), None, ""),
        end_turn(),
    ])?;
    assert_eq!(turn.tool_calls.len(), 1);
    Ok(())
}

#[test]
fn duplicate_index_with_conflicting_id_fails_closed() {
    let err = fold(&[
        delta(0, Some("c1"), Some("bash"), "{}"),
        delta(0, Some("c2"), None, ""),
        end_turn(),
    ]);
    assert!(
        matches!(err, Err(FoldError::DuplicateToolIndex { index: 0 })),
        "conflicting ids on one wire index must fail closed"
    );
}

#[test]
fn empty_string_id_is_treated_as_absent() -> Result<(), FoldError> {
    let turn = fold(&[
        delta(0, Some(""), Some("bash"), "{}"),
        delta(0, Some("c1"), None, ""),
        end_turn(),
    ])?;
    assert_eq!(turn.tool_calls[0].id, "c1");
    Ok(())
}

#[test]
fn truncated_json_arguments_fail_closed() {
    let err = fold(&[delta(0, Some("c1"), Some("bash"), "{\"a\""), end_turn()]);
    assert!(
        matches!(err, Err(FoldError::InvalidArguments { index: 0 })),
        "truncated argument JSON must fail closed"
    );
}

#[test]
fn garbage_json_arguments_fail_closed() {
    let err = fold(&[delta(0, Some("c1"), Some("bash"), "not json"), end_turn()]);
    assert!(matches!(err, Err(FoldError::InvalidArguments { index: 0 })));
}

#[test]
fn empty_arguments_are_allowed_for_no_parameter_calls() -> Result<(), FoldError> {
    let turn = fold(&[delta(0, Some("c1"), Some("clock"), ""), end_turn()])?;
    assert_eq!(turn.tool_calls[0].arguments, "");
    Ok(())
}

#[test]
fn missing_tool_name_fails_closed() {
    let err = fold(&[delta(0, Some("c1"), None, "{}"), end_turn()]);
    assert!(matches!(err, Err(FoldError::MissingToolName { index: 0 })));
}

#[test]
fn events_after_turn_finished_fail_closed() {
    let err = fold(&[end_turn(), text("late")]);
    assert!(matches!(err, Err(FoldError::EventAfterFinished)));
}

#[test]
fn finish_without_terminal_fails_closed() {
    let err = fold(&[text("partial")]);
    assert!(matches!(err, Err(FoldError::Unfinished)));
}

#[test]
fn empty_turn_is_an_error_never_success() -> Result<(), FoldError> {
    let turn = fold(&[end_turn()])?;
    assert!(turn.is_empty());
    let refusal = turn.require_content();
    assert!(
        matches!(refusal, Err(ProviderError::EmptyResponse)),
        "zero-content terminal stop must map to EmptyResponse"
    );
    Ok(())
}

#[test]
fn non_empty_turn_passes_content_check() -> Result<(), FoldError> {
    let turn = fold(&[text("hi"), end_turn()])?;
    assert!(!turn.is_empty());
    assert!(turn.require_content().is_ok());
    Ok(())
}

#[test]
fn tool_only_turn_passes_content_check() -> Result<(), FoldError> {
    let turn = fold(&[delta(0, Some("c1"), Some("bash"), "{}"), end_turn()])?;
    assert!(!turn.is_empty());
    assert!(turn.require_content().is_ok());
    Ok(())
}

#[test]
fn huge_wire_index_never_allocates_by_index() -> Result<(), FoldError> {
    let turn = fold(&[delta(u64::MAX, Some("c1"), Some("bash"), "{}"), end_turn()])?;
    assert_eq!(turn.tool_calls.len(), 1);
    Ok(())
}

#[test]
fn turn_finished_payload_is_preserved() -> Result<(), FoldError> {
    let turn = fold(&[
        text("hi"),
        ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: Usage::omitted(),
        },
    ])?;
    assert_eq!(turn.stop_reason, StopReason::ToolUse);
    assert_eq!(turn.usage, Usage::omitted());
    Ok(())
}

#[test]
fn multibyte_stop_reason_cuts_on_a_char_boundary() -> Result<(), Box<dyn std::error::Error>> {
    let mut fold = TurnFold::new(TurnCaps::default());
    fold.push(ModelEvent::TurnFinished {
        stop_reason: StopReason::Other("あ".repeat(100)),
        usage: Usage::omitted(),
    })?;
    let turn = fold.finish()?;
    let raw = match turn.stop_reason {
        StopReason::Other(raw) => raw,
        other => return Err(format!("stop reason was not Other: {other:?}").into()),
    };
    assert!(raw.len() <= 64, "capped length {}", raw.len());
    assert_eq!(raw.len(), 63);
    assert_eq!(raw.chars().count(), 21);
    Ok(())
}

#[test]
fn ascii_stop_reason_cuts_exactly_at_the_cap() -> Result<(), Box<dyn std::error::Error>> {
    let mut fold = TurnFold::new(TurnCaps::default());
    fold.push(ModelEvent::TurnFinished {
        stop_reason: StopReason::Other("x".repeat(200)),
        usage: Usage::omitted(),
    })?;
    let turn = fold.finish()?;
    let raw = match turn.stop_reason {
        StopReason::Other(raw) => raw,
        other => return Err(format!("stop reason was not Other: {other:?}").into()),
    };
    assert_eq!(raw.len(), 64);
    Ok(())
}

#[test]
fn short_stop_reason_passes_through_unchanged() -> Result<(), FoldError> {
    let mut fold = TurnFold::new(TurnCaps::default());
    fold.push(ModelEvent::TurnFinished {
        stop_reason: StopReason::Other(String::from("overscheduled")),
        usage: Usage::omitted(),
    })?;
    let turn = fold.finish()?;
    assert_eq!(
        turn.stop_reason,
        StopReason::Other(String::from("overscheduled"))
    );
    Ok(())
}
