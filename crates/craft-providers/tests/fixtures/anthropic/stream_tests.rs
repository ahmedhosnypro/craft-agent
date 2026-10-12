//! Stream lifecycle: named events fold into one turn — text and
//! tool-input deltas assemble, thinking content is tolerated, ping and
//! unknown events are skipped, usage merges cumulatively, and exactly
//! one TurnFinished leaves the lane.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{TestError, folded, provider, run, sample_request, terminal_count};
use craft_core::types::{StopReason, Usage};

async fn serve(body: String) -> Result<MockServer, TestError> {
    let server = MockServer::start(vec![CannedResponse::sse(body.as_bytes())], "").await?;
    Ok(server)
}

async fn turn_over(body: String) -> Result<craft_core::events::FoldedTurn, TestError> {
    let server = serve(body).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    crate::folded(&events)
}

#[tokio::test]
async fn the_documented_lifecycle_folds_one_turn() -> Result<(), TestError> {
    let server = serve(wire::text_stream()).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert_eq!(terminal_count(&events), 1, "exactly one TurnFinished");
    let turn = folded(&events)?;
    assert_eq!(turn.text, "Hello world");
    assert_eq!(turn.stop_reason, StopReason::EndTurn);
    Ok(())
}

#[tokio::test]
async fn cumulative_usage_merges_not_sums() -> Result<(), TestError> {
    let server = serve(wire::text_stream()).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let turn = folded(&events)?;
    assert_eq!(
        turn.usage,
        Usage {
            input_tokens: 25,
            output_tokens: 34,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            incomplete: false,
        },
        "message_delta carries the cumulative totals; the max wins"
    );
    Ok(())
}

#[tokio::test]
async fn tool_input_json_deltas_assemble() -> Result<(), TestError> {
    let turn = turn_over(wire::tool_stream()).await?;
    assert_eq!(turn.tool_calls.len(), 1);
    let call = &turn.tool_calls[0];
    assert_eq!(call.id, "toolu_a");
    assert_eq!(call.name, "get_weather");
    assert_eq!(call.arguments, r#"{"city":"Paris"}"#);
    assert_eq!(turn.stop_reason, StopReason::ToolUse);
    Ok(())
}

#[tokio::test]
async fn thinking_blocks_are_tolerated_without_text() -> Result<(), TestError> {
    let turn = turn_over(wire::thinking_stream()).await?;
    assert_eq!(
        turn.text, "hi",
        "thinking and signature deltas never surface as text"
    );
    Ok(())
}

#[tokio::test]
async fn ping_and_unknown_events_are_skipped() -> Result<(), TestError> {
    let server = serve(wire::unknown_event_stream()).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let turn = folded(&events)?;
    assert_eq!(turn.text, "Hel");
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}

#[tokio::test]
async fn bom_prefixed_stream_parses() -> Result<(), TestError> {
    let turn = turn_over(wire::bom_text_stream()).await?;
    assert_eq!(turn.text, "Hello world");
    Ok(())
}

#[tokio::test]
async fn stop_reasons_map_onto_the_closed_vocabulary() -> Result<(), TestError> {
    let rows = [
        ("end_turn", StopReason::EndTurn),
        ("max_tokens", StopReason::MaxTokens),
        ("stop_sequence", StopReason::StopSequence),
        ("tool_use", StopReason::ToolUse),
        ("pause_turn", StopReason::PausedTurn),
        ("refusal", StopReason::Refusal),
        (
            "model_context_window_exceeded",
            StopReason::ContextWindowExceeded,
        ),
    ];
    for (reason, expected) in rows {
        let turn = turn_over(wire::finish_stream(reason)).await?;
        assert_eq!(turn.stop_reason, expected, "stop_reason {reason}");
    }
    let turn = turn_over(wire::finish_stream("brand_new_reason")).await?;
    assert_eq!(
        turn.stop_reason,
        StopReason::Other(String::from("brand_new_reason"))
    );
    Ok(())
}

#[tokio::test]
async fn omitted_usage_flags_the_turn_incomplete() -> Result<(), TestError> {
    let turn = turn_over(wire::no_usage_stream()).await?;
    assert!(
        turn.usage.incomplete,
        "a frame without usage is fail-closed"
    );
    Ok(())
}

#[tokio::test]
async fn zero_content_terminal_is_an_empty_response() -> Result<(), TestError> {
    let server = serve(wire::zero_content_stream()).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let turn = folded(&events)?;
    assert!(turn.is_empty());
    assert_eq!(
        turn.require_content(),
        Err(craft_core::error::ProviderError::EmptyResponse),
        "a zero-content terminal stop is an error, never success"
    );
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}
