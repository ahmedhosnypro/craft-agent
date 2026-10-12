//! Stream lifecycle: named events fold into one turn — text and
//! tool-input deltas assemble, thinking blocks reconstruct for native
//! replay, ping and unknown events are skipped, usage merges
//! cumulatively, and exactly one TurnFinished leaves the lane.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{TestError, folded, provider, run, sample_request, terminal_count};
use craft_core::types::{NativeDialect, StopReason, Usage};

async fn serve(body: String) -> Result<MockServer, TestError> {
    let server = MockServer::start(vec![CannedResponse::sse(body.as_bytes())], "").await?;
    Ok(server)
}

async fn turn_over(body: String) -> Result<craft_core::events::FoldedTurn, TestError> {
    let server = serve(body).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    crate::folded(&events)
}

const EV_START: &str = r#"event: message_start
data: {"type":"message_start","message":{"usage":{"input_tokens":25,"output_tokens":1}}}"#;

const EV_MESSAGE_STOP: &str = "event: message_stop\ndata: {\"type\":\"message_stop\"}";

const EV_FINISH: &str = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}"#;

const EV_TEXT_START_0: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;

const EV_HEL: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#;

const EV_STOP_0: &str = r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#;

const EV_THINK_START_1: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"thinking","thinking":""}}"#;

const EV_THINK_DELTA_1: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#;

const EV_SIG_DELTA_1: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"signature_delta","signature":"Sig9"}}"#;

const EV_STOP_1: &str = r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#;

const EV_TOOL_START_2: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_a","name":"get_weather","input":{}}}"#;

const EV_TOOL_JSON_2: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"city\":\"Paris\"}"}}"#;

const EV_STOP_2: &str = r#"event: content_block_stop
data: {"type":"content_block_stop","index":2}"#;

const EV_FINISH_TOOLS: &str = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null}}"#;

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
async fn thinking_blocks_reconstruct_verbatim_for_replay() -> Result<(), TestError> {
    let turn = turn_over(wire::thinking_stream()).await?;
    let native = turn
        .native_blocks
        .ok_or("the thinking block must be captured")?;
    assert_eq!(native.dialect, NativeDialect::Anthropic);
    assert_eq!(
        native.blocks,
        serde_json::json!([{
            "type": "thinking",
            "thinking": "weighing the request",
            "signature": "EqMC",
        }]),
        "the reconstructed block carries the wire shape verbatim"
    );
    Ok(())
}

#[tokio::test]
async fn redacted_thinking_blocks_pass_through_as_data() -> Result<(), TestError> {
    let start = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"encrypted-blob"}}"#;
    let body = wire::stream(&[EV_START, start, EV_STOP_0, EV_FINISH, EV_MESSAGE_STOP]);
    let turn = turn_over(body).await?;
    let native = turn
        .native_blocks
        .ok_or("the redacted block must be captured")?;
    assert_eq!(native.dialect, NativeDialect::Anthropic);
    assert_eq!(
        native.blocks,
        serde_json::json!([{"type": "redacted_thinking", "data": "encrypted-blob"}])
    );
    Ok(())
}

#[tokio::test]
async fn native_blocks_interleave_with_text_and_tools() -> Result<(), TestError> {
    let body = wire::stream(&[
        EV_START,
        EV_TEXT_START_0,
        EV_HEL,
        EV_STOP_0,
        EV_THINK_START_1,
        EV_THINK_DELTA_1,
        EV_SIG_DELTA_1,
        EV_STOP_1,
        EV_TOOL_START_2,
        EV_TOOL_JSON_2,
        EV_STOP_2,
        EV_FINISH_TOOLS,
        EV_MESSAGE_STOP,
    ]);
    let turn = turn_over(body).await?;
    assert_eq!(turn.text, "Hel");
    assert_eq!(
        turn.tool_calls.len(),
        1,
        "the tool block stands at its own index"
    );
    let call = &turn.tool_calls[0];
    assert_eq!(call.id, "toolu_a");
    assert_eq!(call.name, "get_weather");
    assert_eq!(call.arguments, r#"{"city":"Paris"}"#);
    let native = turn
        .native_blocks
        .ok_or("the interleaved thinking block must be captured")?;
    assert_eq!(
        native.blocks,
        serde_json::json!([{
            "type": "thinking",
            "thinking": "hmm",
            "signature": "Sig9",
        }])
    );
    Ok(())
}

#[tokio::test]
async fn an_empty_thinking_block_emits_no_native_event() -> Result<(), TestError> {
    let body = wire::stream(&[
        EV_START,
        EV_THINK_START_1,
        EV_STOP_1,
        EV_FINISH,
        EV_MESSAGE_STOP,
    ]);
    let turn = turn_over(body).await?;
    assert!(
        turn.native_blocks.is_none(),
        "an empty block is a legal skip"
    );
    assert_eq!(turn.text, "");
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
