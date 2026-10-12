//! Native block folding: provider-native content blocks ride the turn
//! verbatim under hostile-input caps, and a native-only turn is content.

use craft_core::events::{FoldError, FoldedTurn, ModelEvent, TurnCaps, TurnFold};
use craft_core::types::{NativeDialect, StopReason, Usage};

fn native_block(dialect: NativeDialect, thinking: &str) -> ModelEvent {
    ModelEvent::NativeBlock {
        dialect,
        block: serde_json::json!({
            "type": "thinking",
            "thinking": thinking,
            "signature": "c2ln",
        }),
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
fn native_blocks_fold_into_the_turn_verbatim() -> Result<(), FoldError> {
    let turn = fold(&[
        native_block(NativeDialect::Anthropic, "first"),
        native_block(NativeDialect::Anthropic, "second"),
        end_turn(),
    ])?;
    let blocks = turn.native_blocks.as_ref().expect("blocks must pack");
    assert_eq!(blocks.dialect, NativeDialect::Anthropic);
    let array = blocks.blocks.as_array().expect("blocks must be an array");
    assert_eq!(array.len(), 2);
    assert_eq!(array[0]["thinking"], "first");
    assert_eq!(array[1]["thinking"], "second");
    let json = serde_json::to_string(&turn).unwrap();
    let back: FoldedTurn = serde_json::from_str(&json).unwrap();
    assert_eq!(back, turn);
    Ok(())
}

#[test]
fn turn_without_native_blocks_keeps_the_field_absent() -> Result<(), FoldError> {
    let turn = fold(&[end_turn()])?;
    assert!(turn.native_blocks.is_none());
    let json = serde_json::to_string(&turn).unwrap();
    assert!(!json.contains("native_blocks"), "got {json}");
    Ok(())
}

#[test]
fn native_only_turn_is_content_not_an_empty_response() -> Result<(), FoldError> {
    let turn = fold(&[native_block(NativeDialect::Gemini, "plan"), end_turn()])?;
    assert!(!turn.is_empty(), "a thinking-only turn is a real turn");
    assert!(turn.require_content().is_ok());
    let blocks = turn.native_blocks.expect("blocks must pack");
    assert_eq!(blocks.dialect, NativeDialect::Gemini);
    Ok(())
}

#[test]
fn native_blocks_past_the_count_cap_fail_closed() {
    let mut fold = TurnFold::new(TurnCaps::default());
    for _ in 0..64 {
        fold.push(native_block(NativeDialect::Anthropic, "x"))
            .unwrap();
    }
    let err = fold.push(native_block(NativeDialect::Anthropic, "one too many"));
    assert!(
        matches!(err, Err(FoldError::TooManyNativeBlocks { cap: 64 })),
        "the 65th native block must fail closed: {err:?}"
    );
}

#[test]
fn exactly_at_the_count_cap_is_accepted() -> Result<(), FoldError> {
    let mut events: Vec<ModelEvent> = (0..64)
        .map(|_| native_block(NativeDialect::Anthropic, "x"))
        .collect();
    events.push(end_turn());
    let turn = fold(&events)?;
    assert_eq!(
        turn.native_blocks
            .expect("blocks must pack")
            .blocks
            .as_array()
            .map(Vec::len),
        Some(64)
    );
    Ok(())
}

#[test]
fn oversized_native_block_fails_closed() {
    let mut fold = TurnFold::new(TurnCaps::default());
    let oversized = serde_json::json!({"blob": "x".repeat(1024 * 1024)});
    let err = fold.push(ModelEvent::NativeBlock {
        dialect: NativeDialect::Anthropic,
        block: oversized,
    });
    match err {
        Err(FoldError::NativeBlockTooLarge { len, cap }) => {
            assert_eq!(cap, 1024 * 1024);
            assert!(len > cap, "the serialized length must exceed the cap");
        }
        other => panic!("an over-cap block must fail closed: {other:?}"),
    }
}

#[test]
fn mixed_dialects_in_one_turn_fail_closed() {
    let err = fold(&[
        native_block(NativeDialect::Anthropic, "first"),
        native_block(NativeDialect::Gemini, "second"),
        end_turn(),
    ]);
    assert!(
        matches!(err, Err(FoldError::NativeDialectMismatch)),
        "two dialects in one turn must fail closed: {err:?}"
    );
}

#[test]
fn native_block_after_turn_finished_fails_closed() {
    let err = fold(&[end_turn(), native_block(NativeDialect::Anthropic, "late")]);
    assert!(matches!(err, Err(FoldError::EventAfterFinished)));
}
