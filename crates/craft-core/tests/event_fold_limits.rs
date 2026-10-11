//! Fold caps: hostile oversized fields and tool-call counts fail closed
//! with typed errors, never panics.

use craft_core::events::{FoldError, ModelEvent, TurnCaps, TurnFold};
use craft_core::types::{StopReason, Usage};

fn caps() -> TurnCaps {
    TurnCaps {
        max_text_bytes: 16,
        max_tool_calls: 2,
        max_arguments_bytes: 8,
        max_id_bytes: 4,
        max_name_bytes: 6,
    }
}

fn fold_with(caps: TurnCaps, events: &[ModelEvent]) -> Result<(), FoldError> {
    let mut fold = TurnFold::new(caps);
    for event in events {
        fold.push(event.clone())?;
    }
    Ok(())
}

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

#[test]
fn text_exactly_at_cap_is_accepted() {
    let result = fold_with(caps(), &[text("0123456789abcdef"), end_turn()]);
    assert!(result.is_ok(), "exact-cap text must pass, got {result:?}");
}

#[test]
fn text_over_cap_fails_closed() {
    let err = fold_with(caps(), &[text("0123456789abcdef"), text("x"), end_turn()]);
    assert!(
        matches!(
            err,
            Err(FoldError::FieldTooLarge {
                field: "text",
                cap: 16,
                ..
            })
        ),
        "oversized text must fail closed"
    );
}

#[test]
fn multibyte_text_over_cap_fails_closed_without_panic() {
    // Byte cap cuts inside a 3-byte character; the check is on bytes and
    // never slices, so this must be a typed error.
    let err = fold_with(caps(), &[text("あいうえお"), text("あ"), end_turn()]);
    assert!(
        matches!(err, Err(FoldError::FieldTooLarge { field: "text", .. })),
        "multibyte overflow must be a typed error"
    );
}

#[test]
fn single_oversized_text_fragment_fails_closed() {
    let err = fold_with(caps(), &[text(&"y".repeat(1024)), end_turn()]);
    assert!(matches!(
        err,
        Err(FoldError::FieldTooLarge { field: "text", .. })
    ));
}

#[test]
fn arguments_over_cap_fails_closed() {
    let err = fold_with(
        caps(),
        &[
            delta(0, Some("c1"), Some("bash"), "{\"a\":1}"),
            delta(0, None, None, "xy"),
            end_turn(),
        ],
    );
    assert!(
        matches!(
            err,
            Err(FoldError::FieldTooLarge {
                field: "tool arguments",
                cap: 8,
                ..
            })
        ),
        "oversized arguments must fail closed"
    );
}

#[test]
fn single_oversized_argument_fragment_fails_closed() {
    let err = fold_with(
        caps(),
        &[
            delta(0, Some("c1"), Some("bash"), &"z".repeat(9999)),
            end_turn(),
        ],
    );
    assert!(matches!(
        err,
        Err(FoldError::FieldTooLarge {
            field: "tool arguments",
            ..
        })
    ));
}

#[test]
fn name_over_cap_fails_closed() {
    let err = fold_with(
        caps(),
        &[
            delta(0, Some("c1"), Some("bash"), "{}"),
            delta(0, None, Some("cmd"), ""),
            end_turn(),
        ],
    );
    assert!(
        matches!(
            err,
            Err(FoldError::FieldTooLarge {
                field: "tool name",
                cap: 6,
                ..
            })
        ),
        "oversized name must fail closed"
    );
}

#[test]
fn id_over_cap_fails_closed() {
    let err = fold_with(
        caps(),
        &[delta(0, Some("call_12345"), Some("bash"), "{}"), end_turn()],
    );
    assert!(
        matches!(
            err,
            Err(FoldError::FieldTooLarge {
                field: "tool id",
                cap: 4,
                ..
            })
        ),
        "oversized id must fail closed"
    );
}

#[test]
fn tool_call_count_over_cap_fails_closed() {
    let err = fold_with(
        caps(),
        &[
            delta(0, Some("c1"), Some("one"), "{}"),
            delta(1, Some("c2"), Some("two"), "{}"),
            delta(2, Some("c3"), Some("three"), "{}"),
            end_turn(),
        ],
    );
    assert!(
        matches!(err, Err(FoldError::TooManyToolCalls { cap: 2 })),
        "tool-call count must be capped"
    );
}

#[test]
fn distinct_wire_indices_do_not_double_count_fragments() {
    // Two calls, many fragments each: only distinct indices count.
    let result = fold_with(
        caps(),
        &[
            delta(0, Some("c1"), Some("bash"), "{\"a\""),
            delta(1, Some("c2"), Some("grep"), "{\"b\""),
            delta(0, None, None, ":1}"),
            delta(1, None, None, ":2}"),
            end_turn(),
        ],
    );
    assert!(
        result.is_ok(),
        "fragment reuse must not count twice, got {result:?}"
    );
}
