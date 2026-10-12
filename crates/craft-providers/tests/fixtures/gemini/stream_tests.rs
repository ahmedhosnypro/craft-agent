//! SSE decode: candidate parts fold into a turn, whole function calls
//! land per part, cumulative usageMetadata merges into one terminal, and
//! finishReason maps onto the closed stop vocabulary.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{TestError, folded, provider, run, sample_request, terminal_count};
use craft_core::types::{StopReason, ToolCall, Usage};

async fn serve(body: String) -> Result<MockServer, TestError> {
    let server = MockServer::start(vec![CannedResponse::sse(body.as_bytes())], "").await?;
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
async fn content_deltas_and_cumulative_usage_fold() -> Result<(), TestError> {
    let (turn, events) = turn_over(wire::text_stream()).await?;
    assert_eq!(turn.text, "Hello world");
    assert_eq!(turn.stop_reason, StopReason::EndTurn);
    assert_eq!(
        turn.usage,
        Usage {
            input_tokens: 8,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_write_tokens: 0,
            reasoning_tokens: 4,
            incomplete: false,
        },
        "the terminal chunk's cumulative totals win per bucket"
    );
    let spent = turn.usage.input_tokens
        + turn.usage.output_tokens
        + turn.usage.cache_read_tokens
        + turn.usage.cache_write_tokens
        + turn.usage.reasoning_tokens;
    assert_eq!(spent, 17, "disjoint buckets sum to totalTokenCount");
    assert_eq!(
        terminal_count(&events),
        1,
        "the stream yields exactly one TurnFinished: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn thought_parts_stay_out_of_the_text() -> Result<(), TestError> {
    let (turn, events) = turn_over(wire::reasoning_stream()).await?;
    assert_eq!(
        turn.text, "Hello",
        "thought parts and bare signatures never join the answer text"
    );
    assert_eq!(turn.usage.reasoning_tokens, 5);
    assert_eq!(turn.usage.input_tokens, 9);
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}

#[tokio::test]
async fn function_calls_arrive_whole_per_part() -> Result<(), TestError> {
    let (turn, events) = turn_over(wire::tool_stream()).await?;
    assert_eq!(turn.text, "");
    assert_eq!(turn.stop_reason, StopReason::EndTurn);
    assert_eq!(
        turn.tool_calls,
        vec![
            ToolCall {
                id: String::from("call_a"),
                name: String::from("get_weather"),
                arguments: String::from("{\"city\":\"Paris\"}"),
            },
            ToolCall {
                id: String::new(),
                name: String::from("bash"),
                arguments: String::from("{\"cmd\":\"ls\"}"),
            },
        ],
        "a wire id rides the call; a missing one folds to the empty id"
    );
    assert_eq!(turn.usage.input_tokens, 42);
    assert_eq!(turn.usage.output_tokens, 9);
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}

#[tokio::test]
async fn signature_bearing_calls_still_fold() -> Result<(), TestError> {
    let (turn, _) = turn_over(wire::signature_tool_stream()).await?;
    assert_eq!(
        turn.tool_calls,
        vec![ToolCall {
            id: String::from("call_sig"),
            name: String::from("lookup"),
            arguments: String::from("{\"city\":\"Oslo\"}"),
        }],
        "a thoughtSignature on the call part is tolerated, not the call"
    );
    Ok(())
}

#[tokio::test]
async fn finish_reasons_map_onto_the_closed_vocabulary() -> Result<(), TestError> {
    let rows = [
        ("STOP", StopReason::EndTurn),
        ("MAX_TOKENS", StopReason::MaxTokens),
        ("SAFETY", StopReason::ContentFilter),
        ("RECITATION", StopReason::ContentFilter),
        ("BLOCKLIST", StopReason::ContentFilter),
        ("SPII", StopReason::ContentFilter),
        ("MALFORMED_FUNCTION_CALL", StopReason::MalformedToolCall),
        ("UNEXPECTED_TOOL_CALL", StopReason::MalformedToolCall),
        ("ESCALATION", StopReason::Other(String::from("ESCALATION"))),
    ];
    for (reason, expected) in rows {
        let (turn, _) = turn_over(wire::finish_stream(reason)).await?;
        assert_eq!(turn.stop_reason, expected, "wire reason {reason}");
    }
    Ok(())
}

#[tokio::test]
async fn trailing_usage_frame_keeps_one_terminal() -> Result<(), TestError> {
    let (turn, events) = turn_over(wire::accounting_frame_stream()).await?;
    assert_eq!(turn.text, "Hello world");
    assert_eq!(turn.stop_reason, StopReason::EndTurn);
    assert_eq!(turn.usage.input_tokens, 14, "per-bucket max across chunks");
    assert_eq!(turn.usage.output_tokens, 3);
    assert_eq!(turn.usage.reasoning_tokens, 4);
    assert_eq!(turn.usage.cache_read_tokens, 3);
    assert!(!turn.usage.incomplete);
    assert_eq!(
        terminal_count(&events),
        1,
        "the usage-only frame must never become a second terminal: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn omitted_usage_flags_the_turn_incomplete() -> Result<(), TestError> {
    let (turn, events) = turn_over(wire::no_usage_stream()).await?;
    assert_eq!(turn.text, "hi");
    assert_eq!(turn.stop_reason, StopReason::MaxTokens);
    assert!(turn.usage.incomplete, "no usageMetadata arrived");
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}

#[tokio::test]
async fn blocked_prompt_finishes_content_filtered() -> Result<(), TestError> {
    let (turn, events) = turn_over(wire::prompt_feedback_stream()).await?;
    assert_eq!(
        turn.stop_reason,
        StopReason::ContentFilter,
        "a promptFeedback blockReason is the terminal stop"
    );
    assert!(turn.is_empty(), "a blocked prompt carries no content");
    assert_eq!(
        turn.require_content(),
        Err(craft_core::error::ProviderError::EmptyResponse),
        "the zero-content block surfaces as EmptyResponse, never success"
    );
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}

#[tokio::test]
async fn zero_content_terminal_folds_empty() -> Result<(), TestError> {
    let (turn, _) = turn_over(wire::zero_content_stream()).await?;
    assert!(turn.is_empty());
    assert_eq!(
        turn.require_content(),
        Err(craft_core::error::ProviderError::EmptyResponse),
        "a zero-content terminal stop is an error, never success"
    );
    Ok(())
}

#[tokio::test]
async fn bom_prefixed_stream_parses() -> Result<(), TestError> {
    let (turn, _) = turn_over(wire::bom_text_stream()).await?;
    assert_eq!(turn.text, "Hello world");
    assert_eq!(turn.usage.input_tokens, 8);
    assert!(!turn.usage.incomplete);
    Ok(())
}
