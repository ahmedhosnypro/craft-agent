//! Hostile rows: truncated chunks, invalid UTF-8, corrupt ordering, and
//! mid-stream error envelopes map to typed errors, never panics, and
//! hostile tool fragments fail closed in the fold.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{
    Outcome, TestError, detail_of, fold, fold_with_caps, provider, run, sample_request, surfaced,
    terminal_count,
};
use craft_core::error::ProviderError;
use craft_core::events::{FoldError, ModelEvent, TurnCaps};
use craft_core::types::Usage;

async fn serve(body: &[u8]) -> Result<MockServer, TestError> {
    let server = MockServer::start(vec![CannedResponse::sse(body)]).await?;
    Ok(server)
}

async fn events_over(body: &[u8]) -> Result<Vec<Outcome>, TestError> {
    let server = serve(body).await?;
    Ok(run(&provider(&server.base_url)?, sample_request()).await)
}

#[tokio::test]
async fn truncated_json_chunk_maps_to_transport() -> Result<(), TestError> {
    let events = events_over(wire::truncated_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("chunk"));
    Ok(())
}

#[tokio::test]
async fn invalid_utf8_maps_to_transport() -> Result<(), TestError> {
    let events = events_over(wire::INVALID_UTF8_STREAM).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("UTF-8"));
    Ok(())
}

#[tokio::test]
async fn oversized_sse_event_maps_to_transport() -> Result<(), TestError> {
    let body = format!("{}\n\n", wire::oversized_chunk(&"x".repeat(1_100_000)));
    let events = events_over(body.as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("exceeded"));
    Ok(())
}

#[tokio::test]
async fn done_without_a_finish_is_stream_closed() -> Result<(), TestError> {
    let events = events_over(wire::done_only_stream().as_bytes()).await?;
    assert_eq!(surfaced(&events)?, ProviderError::StreamClosed);
    Ok(())
}

#[tokio::test]
async fn eof_without_a_finish_is_stream_closed() -> Result<(), TestError> {
    let events = events_over(wire::unfinished_stream().as_bytes()).await?;
    assert!(
        matches!(events.first(), Some(Ok(ModelEvent::TextDelta { .. }))),
        "the content before the cut still streams: {events:?}"
    );
    assert_eq!(surfaced(&events)?, ProviderError::StreamClosed);
    Ok(())
}

#[tokio::test]
async fn conflicting_finish_reason_fails_closed() -> Result<(), TestError> {
    let events = events_over(wire::conflicting_finish_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("conflicting finish_reason"));
    Ok(())
}

#[tokio::test]
async fn content_after_the_finish_is_corrupt() -> Result<(), TestError> {
    let events = events_over(wire::late_content_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("after finish_reason"));
    Ok(())
}

#[tokio::test]
async fn midstream_server_error_maps_transient() -> Result<(), TestError> {
    let body = wire::midstream_error_stream("server_error", None);
    let events = events_over(body.as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transient { .. }), "{err:?}");
    Ok(())
}

#[tokio::test]
async fn midstream_rate_error_maps_rate_limit() -> Result<(), TestError> {
    let body = wire::midstream_error_stream("rate_limit_error", None);
    let events = events_over(body.as_bytes()).await?;
    let err = surfaced(&events)?;
    assert_eq!(err, ProviderError::RateLimit { retry_after: None });
    Ok(())
}

#[tokio::test]
async fn midstream_context_code_maps_context_window() -> Result<(), TestError> {
    let body =
        wire::midstream_error_stream("invalid_request_error", Some("context_length_exceeded"));
    let events = events_over(body.as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(
        matches!(err, ProviderError::ContextWindow { .. }),
        "{err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn duplicate_tool_indices_fail_in_the_fold() -> Result<(), TestError> {
    let events = events_over(wire::duplicate_index_stream().as_bytes()).await?;
    assert!(
        events.iter().all(|event| event.is_ok()),
        "the adapter streams both fragments without judgment: {events:?}"
    );
    assert_eq!(
        fold(&events)?,
        Err(FoldError::DuplicateToolIndex { index: 0 }),
        "the shared fold fails closed on the hostile pair"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_tool_arguments_fail_in_the_fold() -> Result<(), TestError> {
    let events = events_over(wire::bad_arguments_stream().as_bytes()).await?;
    assert_eq!(
        fold(&events)?,
        Err(FoldError::InvalidArguments { index: 0 }),
        "concatenated fragments that are not JSON fail closed"
    );
    Ok(())
}

#[tokio::test]
async fn oversized_fields_hit_the_fold_caps() -> Result<(), TestError> {
    let body = format!("{}\n\n", wire::oversized_chunk(&"x".repeat(4096)));
    let events = events_over(body.as_bytes()).await?;
    let caps = TurnCaps {
        max_text_bytes: 1024,
        ..TurnCaps::default()
    };
    let outcome = fold_with_caps(&events, caps)?;
    assert!(
        matches!(outcome, Err(FoldError::FieldTooLarge { field: "text", .. })),
        "hostile size fails at the shared caps: {outcome:?}"
    );
    Ok(())
}

#[tokio::test]
async fn wire_end_after_the_finish_preserves_the_turn() -> Result<(), TestError> {
    let events = events_over(wire::bare_finish_stream().as_bytes()).await?;
    assert!(
        events.iter().all(|event| event.is_ok()),
        "a clean close after the finish is not an error: {events:?}"
    );
    assert_eq!(terminal_count(&events), 1);
    let turn = crate::folded(&events)?;
    assert_eq!(turn.stop_reason, craft_core::types::StopReason::EndTurn);
    assert_eq!(
        turn.usage,
        Usage::omitted(),
        "the missing accounting frame flags the turn incomplete"
    );
    Ok(())
}
