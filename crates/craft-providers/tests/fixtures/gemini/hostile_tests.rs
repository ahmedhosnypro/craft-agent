//! Hostile rows: truncated chunks, invalid UTF-8, corrupt ordering, and
//! oversized fields map to typed errors, never panics; hostile sizes
//! fail closed at the shared fold caps.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{
    Outcome, TestError, detail_of, fold_with_caps, provider, run, sample_request, surfaced,
    terminal_count,
};
use craft_core::error::ProviderError;
use craft_core::events::{FoldError, ModelEvent, TurnCaps};

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
    assert!(detail_of(&err).contains("conflicting finishReason"));
    Ok(())
}

#[tokio::test]
async fn content_after_the_finish_is_corrupt() -> Result<(), TestError> {
    let events = events_over(wire::late_content_stream().as_bytes()).await?;
    let err = surfaced(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    assert!(detail_of(&err).contains("after finishReason"));
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
async fn oversized_tool_arguments_hit_the_fold_caps() -> Result<(), TestError> {
    let body = format!("{}\n\n", wire::oversized_args_chunk(&"x".repeat(4096)));
    let events = events_over(body.as_bytes()).await?;
    let caps = TurnCaps {
        max_arguments_bytes: 1024,
        ..TurnCaps::default()
    };
    let outcome = fold_with_caps(&events, caps)?;
    assert!(
        matches!(
            outcome,
            Err(FoldError::FieldTooLarge {
                field: "tool arguments",
                ..
            })
        ),
        "a hostile args blob fails at the shared caps: {outcome:?}"
    );
    Ok(())
}

#[tokio::test]
async fn too_many_tool_calls_fail_in_the_fold() -> Result<(), TestError> {
    let calls: Vec<String> = (0..300)
        .map(|index| format!(r#"{{"functionCall":{{"name":"f{index}","args":{{}}}}}}"#))
        .collect();
    let chunk = format!(
        r#"data: {{"candidates":[{{"content":{{"parts":[{}],"role":"model"}},"finishReason":"STOP","index":0}}]}}"#,
        calls.join(",")
    );
    let events = events_over(format!("{chunk}\n\n").as_bytes()).await?;
    assert_eq!(
        fold_with_caps(&events, TurnCaps::default())?,
        Err(FoldError::TooManyToolCalls { cap: 256 }),
        "a hostile call fan-out fails closed at the shared cap"
    );
    Ok(())
}

#[tokio::test]
async fn unknown_chunk_fields_are_ignored() -> Result<(), TestError> {
    let chunk = r#"data: {"candidates":[{"content":{"parts":[{"text":"ok"}],"role":"model"},"finishReason":"STOP","index":0,"safetyRatings":[{"category":"HARM_CATEGORY_HARASSMENT","probability":"NEGLIGIBLE"}],"tokenCount":2,"citationMetadata":{"citations":[]}}],"promptFeedback":{"safetyRatings":[]},"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2},"modelVersion":"gemini-2.5-flash","responseId":"r1"}"#;
    let events = events_over(format!("{chunk}\n\n").as_bytes()).await?;
    let turn = crate::folded(&events)?;
    assert_eq!(turn.text, "ok");
    assert_eq!(turn.usage.input_tokens, 1);
    assert_eq!(terminal_count(&events), 1);
    Ok(())
}
