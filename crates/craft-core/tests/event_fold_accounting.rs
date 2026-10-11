//! Accounting frames: a usage chunk repeating the terminal stop reason is
//! bookkeeping, never a second terminal; its usage merges and a
//! conflicting reason fails closed.

use craft_core::events::{FoldError, ModelEvent, TurnCaps, TurnFold};
use craft_core::types::{StopReason, Usage};

fn finished(stop_reason: StopReason, input: u64, output: u64) -> ModelEvent {
    ModelEvent::TurnFinished {
        stop_reason,
        usage: Usage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            incomplete: false,
        },
    }
}

#[test]
fn repeated_terminal_with_same_reason_merges_usage() -> Result<(), FoldError> {
    let mut fold = TurnFold::new(TurnCaps::default());
    fold.push(finished(StopReason::ToolUse, 10, 0))?;
    fold.push(finished(StopReason::ToolUse, 10, 6))?;
    let turn = fold.finish()?;
    assert_eq!(turn.stop_reason, StopReason::ToolUse, "one terminal only");
    assert_eq!(turn.usage.input_tokens, 10);
    assert_eq!(turn.usage.output_tokens, 6);
    Ok(())
}

#[test]
fn repeated_terminal_is_idempotent_for_identical_usage() -> Result<(), FoldError> {
    let mut fold = TurnFold::new(TurnCaps::default());
    fold.push(finished(StopReason::EndTurn, 4, 2))?;
    fold.push(finished(StopReason::EndTurn, 4, 2))?;
    let turn = fold.finish()?;
    assert_eq!(turn.usage.input_tokens, 4);
    assert_eq!(turn.usage.output_tokens, 2);
    assert!(!turn.usage.incomplete);
    Ok(())
}

#[test]
fn repeated_terminal_with_unknown_reason_string_absorbs_same_text() -> Result<(), FoldError> {
    let mut fold = TurnFold::new(TurnCaps::default());
    fold.push(finished(StopReason::Other(String::from("error")), 1, 1))?;
    fold.push(finished(StopReason::Other(String::from("error")), 1, 2))?;
    let turn = fold.finish()?;
    assert_eq!(
        turn.stop_reason,
        StopReason::Other(String::from("error")),
        "the repeated gateway finish reason stays one terminal"
    );
    assert_eq!(turn.usage.output_tokens, 2);
    Ok(())
}

#[test]
fn repeated_terminal_with_different_reason_fails_closed() {
    let mut fold = TurnFold::new(TurnCaps::default());
    let first = fold.push(finished(StopReason::EndTurn, 1, 1));
    assert!(first.is_ok());
    let second = fold.push(finished(StopReason::MaxTokens, 1, 2));
    assert!(
        matches!(second, Err(FoldError::EventAfterFinished)),
        "a conflicting repeated reason is a corrupt stream"
    );
}

#[test]
fn omitted_marker_on_repeat_does_not_flag_incomplete() -> Result<(), FoldError> {
    let mut fold = TurnFold::new(TurnCaps::default());
    fold.push(finished(StopReason::EndTurn, 9, 3))?;
    let repeat = ModelEvent::TurnFinished {
        stop_reason: StopReason::EndTurn,
        usage: Usage::omitted(),
    };
    fold.push(repeat)?;
    let turn = fold.finish()?;
    assert!(
        !turn.usage.incomplete,
        "the omitted marker carries no accounting"
    );
    assert_eq!(turn.usage.input_tokens, 9);
    Ok(())
}
