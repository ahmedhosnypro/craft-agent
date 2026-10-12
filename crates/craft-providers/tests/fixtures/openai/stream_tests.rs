//! SSE decode: deltas fold into a turn, tool fragments assemble by wire
//! index, accounting frames merge into one terminal, and finish reasons
//! map onto the closed stop vocabulary.

use crate::mock::{CannedResponse, MockServer};
use crate::{TestError, folded, provider, run, sample_request, terminal_count};
use craft_core::types::{StopReason, ToolCall, Usage};

async fn serve(body: String) -> Result<MockServer, TestError> {
    let server = MockServer::start(vec![CannedResponse::sse(body.as_bytes())], "/v1").await?;
    Ok(server)
}

async fn turn_over(
    body: String,
) -> Result<(craft_core::events::FoldedTurn, Vec<crate::Outcome>), TestError> {
    let server = serve(body).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let turn = folded(&events)?;
    Ok((turn, events))
}

#[tokio::test]
async fn content_deltas_and_accounting_frame_fold() -> Result<(), TestError> {
    let (turn, events) = turn_over(crate::wire::text_stream()).await?;
    assert_eq!(turn.text, "Hello world");
    assert_eq!(turn.stop_reason, StopReason::EndTurn);
    assert_eq!(
        turn.usage,
        Usage {
            input_tokens: 11,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            incomplete: false,
        }
    );
    assert_eq!(
        terminal_count(&events),
        1,
        "the usage chunk must never become a second terminal: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn tool_fragments_assemble_by_wire_index() -> Result<(), TestError> {
    let (turn, events) = turn_over(crate::wire::tool_stream()).await?;
    assert_eq!(turn.stop_reason, StopReason::ToolUse);
    assert_eq!(turn.text, "");
    assert_eq!(
        turn.tool_calls,
        vec![
            ToolCall {
                id: String::from("call_a"),
                name: String::from("get_weather"),
                arguments: String::from("{\"city\":\"Paris\"}"),
            },
            ToolCall {
                id: String::from("call_b"),
                name: String::from("bash"),
                arguments: String::from("{\"cmd\":\"ls\"}"),
            },
        ]
    );
    assert_eq!(turn.usage.input_tokens, 35);
    assert_eq!(turn.usage.output_tokens, 5);
    assert_eq!(turn.usage.cache_read_tokens, 7);
    assert_eq!(turn.usage.reasoning_tokens, 4);
    assert!(!turn.usage.incomplete);
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}

#[tokio::test]
async fn usage_buckets_stay_disjoint_so_their_sum_is_the_wire_total() -> Result<(), TestError> {
    let (turn, _) = turn_over(crate::wire::tool_stream()).await?;
    let spent = turn.usage.input_tokens
        + turn.usage.output_tokens
        + turn.usage.cache_read_tokens
        + turn.usage.cache_write_tokens
        + turn.usage.reasoning_tokens;
    assert_eq!(spent, 51, "cached and reasoning ride their own buckets");
    Ok(())
}

#[tokio::test]
async fn non_empty_choices_usage_repeat_is_one_terminal() -> Result<(), TestError> {
    let (turn, events) = turn_over(crate::wire::accounting_repeat_stream()).await?;
    assert_eq!(turn.text, "hi");
    assert_eq!(turn.stop_reason, StopReason::EndTurn);
    assert_eq!(turn.usage.input_tokens, 9);
    assert_eq!(turn.usage.output_tokens, 3);
    assert!(!turn.usage.incomplete);
    assert_eq!(
        terminal_count(&events),
        1,
        "the repeat finish reason is an accounting frame: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn finish_reasons_map_onto_the_closed_vocabulary() -> Result<(), TestError> {
    let rows = [
        ("stop", StopReason::EndTurn),
        ("length", StopReason::MaxTokens),
        ("tool_calls", StopReason::ToolUse),
        ("function_call", StopReason::ToolUse),
        ("content_filter", StopReason::ContentFilter),
        ("STOP", StopReason::EndTurn),
        ("error", StopReason::Other(String::from("error"))),
    ];
    for (reason, expected) in rows {
        let (turn, _) = turn_over(crate::wire::finish_stream(reason)).await?;
        assert_eq!(turn.stop_reason, expected, "wire reason {reason}");
    }
    Ok(())
}

#[tokio::test]
async fn omitted_usage_flags_the_turn_incomplete() -> Result<(), TestError> {
    let (turn, events) = turn_over(crate::wire::no_usage_stream()).await?;
    assert_eq!(turn.text, "hi");
    assert_eq!(turn.stop_reason, StopReason::MaxTokens);
    assert!(turn.usage.incomplete, "no usage chunk arrived");
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}

#[tokio::test]
async fn bom_prefixed_stream_parses() -> Result<(), TestError> {
    let (turn, _) = turn_over(crate::wire::bom_text_stream()).await?;
    assert_eq!(turn.text, "Hello world");
    assert_eq!(turn.usage.input_tokens, 11);
    assert!(!turn.usage.incomplete);
    Ok(())
}

#[tokio::test]
async fn zero_content_terminal_folds_empty() -> Result<(), TestError> {
    let (turn, _) = turn_over(crate::wire::zero_content_stream()).await?;
    assert!(turn.is_empty());
    assert_eq!(
        turn.require_content(),
        Err(craft_core::error::ProviderError::EmptyResponse),
        "a zero-content terminal stop is an error, never success"
    );
    Ok(())
}
