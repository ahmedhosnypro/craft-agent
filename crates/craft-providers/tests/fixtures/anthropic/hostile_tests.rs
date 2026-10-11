//! Hostile rows: truncated fragments, invalid UTF-8, corrupt ordering,
//! and mid-stream error events map to typed errors, never panics, and
//! hostile tool fragments fail closed in the fold.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{
    Outcome, TestError, detail_of, fold_with_caps, provider, run, sample_request, surfaced,
    terminal_count,
};
use craft_core::error::ProviderError;
use craft_core::events::TurnCaps;

async fn serve(body: &[u8]) -> Result<MockServer, TestError> {
    let server = MockServer::start(vec![CannedResponse::sse(body)]).await?;
    Ok(server)
}

async fn events_over(body: &[u8]) -> Result<Vec<Outcome>, TestError> {
    let server = serve(body).await?;
    Ok(run(&provider(&server.base_url)?, sample_request()).await)
}

#[tokio::test]
async fn truncated_json_event_maps_to_transport() -> Result<(), TestError> {
    let events = events_over(wire::truncated_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("event"));
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
async fn orphan_delta_without_a_start_is_corrupt() -> Result<(), TestError> {
    let events = events_over(wire::orphan_delta_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("content_block_start"));
    Ok(())
}

#[tokio::test]
async fn message_stop_without_a_terminal_is_stream_closed() -> Result<(), TestError> {
    let events = events_over(wire::message_stop_only_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert_eq!(err, ProviderError::StreamClosed);
    Ok(())
}

#[tokio::test]
async fn eof_without_a_terminal_is_stream_closed() -> Result<(), TestError> {
    let events = events_over(wire::unfinished_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert_eq!(err, ProviderError::StreamClosed);
    Ok(())
}

#[tokio::test]
async fn content_after_the_terminal_is_corrupt() -> Result<(), TestError> {
    let events = events_over(wire::late_content_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    Ok(())
}

#[tokio::test]
async fn wire_end_after_the_finish_preserves_the_turn() -> Result<(), TestError> {
    let events = events_over(wire::wire_end_after_finish_stream().as_bytes()).await?;
    assert_eq!(terminal_count(&events), 1);
    let turn = crate::folded(&events)?;
    assert_eq!(turn.text, "Hel");
    Ok(())
}

#[tokio::test]
async fn midstream_overloaded_error_maps_transient() -> Result<(), TestError> {
    let events = events_over(wire::midstream_error_stream("overloaded_error").as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(
        matches!(err, ProviderError::Transient { .. }),
        "got {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn midstream_rate_error_maps_rate_limit() -> Result<(), TestError> {
    let events = events_over(wire::midstream_error_stream("rate_limit_error").as_bytes()).await?;
    let err = surfaced(&events)?;
    assert_eq!(err, ProviderError::RateLimit { retry_after: None });
    Ok(())
}

#[tokio::test]
async fn midstream_invalid_request_maps_invalid_request() -> Result<(), TestError> {
    let events =
        events_over(wire::midstream_error_stream("invalid_request_error").as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "got {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn midstream_authentication_error_maps_auth() -> Result<(), TestError> {
    let events =
        events_over(wire::midstream_error_stream("authentication_error").as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Auth { .. }), "got {err:?}");
    Ok(())
}

#[tokio::test]
async fn duplicate_tool_indices_fail_in_the_fold() -> Result<(), TestError> {
    let events = events_over(wire::duplicate_tool_index_stream().as_bytes()).await?;
    let outcome = fold_with_caps(&events, TurnCaps::default())?;
    let err = outcome.err().ok_or("conflicting ids must fail the fold")?;
    assert!(
        matches!(
            err,
            craft_core::events::FoldError::DuplicateToolIndex { index: 0 }
        ),
        "got {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_tool_arguments_fail_in_the_fold() -> Result<(), TestError> {
    let events = events_over(wire::bad_tool_arguments_stream().as_bytes()).await?;
    let outcome = fold_with_caps(&events, TurnCaps::default())?;
    let err = outcome
        .err()
        .ok_or("non-JSON arguments must fail the fold")?;
    assert!(
        matches!(
            err,
            craft_core::events::FoldError::InvalidArguments { index: 0 }
        ),
        "got {err:?}"
    );
    Ok(())
}

#[tokio::test]
async fn oversized_text_hits_the_fold_caps() -> Result<(), TestError> {
    let body = wire::oversized_text_stream(&"x".repeat(64));
    let events = events_over(body.as_bytes()).await?;
    let caps = TurnCaps {
        max_text_bytes: 32,
        ..TurnCaps::default()
    };
    let outcome = fold_with_caps(&events, caps)?;
    let err = outcome
        .err()
        .ok_or("the oversized text must fail the fold")?;
    assert!(
        matches!(
            err,
            craft_core::events::FoldError::FieldTooLarge { field: "text", .. }
        ),
        "got {err:?}"
    );
    Ok(())
}
