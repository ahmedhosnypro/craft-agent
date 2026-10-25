//! HTTP error rows: auth fails fast, 429 honors the live Retry-After and
//! surfaces after the streak, 5xx climbs the ladder, and error bodies
//! refine the closed taxonomy. Paused time keeps the ladder instant.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{KEY, MODEL};
use crate::{Outcome, TestError, detail_of, provider, run, sample_request};
use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::types::StopReason;
use craft_providers::openai::OpenAiProvider;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn surfaced_error(events: &[Outcome]) -> Result<ProviderError, TestError> {
    match events.first() {
        Some(Err(err)) => Ok(err.clone()),
        _ => Err(String::from("the row must surface one typed error").into()),
    }
}

/// Serves the canned rows and yields the mock with the surfaced error.
async fn error_over(
    responses: Vec<CannedResponse>,
) -> Result<(MockServer, ProviderError), TestError> {
    let server = MockServer::start(responses, "/v1").await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    Ok((server, surfaced_error(&events)?))
}

/// One canned error row: the typed variant, its detail substring, and a
/// single request — the row never rides the ladder.
async fn fail_fast_row(
    status: u16,
    body: &str,
    is_variant: impl Fn(&ProviderError) -> bool,
    detail: &str,
) -> Result<(), TestError> {
    let (server, err) = error_over(vec![CannedResponse::json(status, body)]).await?;
    assert!(is_variant(&err), "got {err:?}");
    assert!(detail_of(&err).contains(detail));
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn auth_401_fails_fast_without_retry() -> Result<(), TestError> {
    fail_fast_row(
        401,
        wire::AUTH_BODY,
        |err| matches!(err, ProviderError::Auth { .. }),
        "Incorrect API key",
    )
    .await
}

#[tokio::test(start_paused = true)]
async fn auth_403_fails_fast_without_retry() -> Result<(), TestError> {
    let (server, err) = error_over(vec![CannedResponse::json(403, wire::FORBIDDEN_BODY)]).await?;
    assert!(matches!(err, ProviderError::Auth { .. }), "got {err:?}");
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn rate_limit_streak_honors_the_live_retry_after() -> Result<(), TestError> {
    let responses = (0..5)
        .map(|_| CannedResponse::json(429, wire::RATE_BODY).with_header("retry-after", "3"))
        .collect();
    let (server, err) = error_over(responses).await?;
    assert_eq!(
        err,
        ProviderError::RateLimit {
            retry_after: Some(Duration::from_secs(3)),
        },
        "the surfaced error carries the last live Retry-After"
    );
    assert_eq!(
        server.request_count(),
        5,
        "the consecutive-429 streak surfaces before the ladder runs dry"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn rate_limit_without_header_falls_back_to_the_ladder() -> Result<(), TestError> {
    let responses = (0..5)
        .map(|_| CannedResponse::json(429, wire::RATE_BODY))
        .collect();
    let (server, err) = error_over(responses).await?;
    assert_eq!(err, ProviderError::RateLimit { retry_after: None });
    assert_eq!(server.request_count(), 5);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn quota_429_fails_fast_without_retry() -> Result<(), TestError> {
    fail_fast_row(
        429,
        wire::QUOTA_BODY,
        |err| matches!(err, ProviderError::QuotaExhausted { .. }),
        "exceeded your current quota",
    )
    .await
}

#[tokio::test(start_paused = true)]
async fn server_errors_climb_the_full_ladder() -> Result<(), TestError> {
    let responses = (0..6)
        .map(|_| CannedResponse::json(500, wire::SERVER_BODY))
        .collect();
    let server = MockServer::start(responses, "/v1").await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::Transient { .. }),
        "persistent 5xx surfaces as Transient: {err:?}"
    );
    assert_eq!(
        server.request_count(),
        6,
        "the ladder is bounded by the shared constant"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn mixed_failures_reset_the_rate_streak() -> Result<(), TestError> {
    let statuses = [500, 429, 500, 429, 500, 429];
    let responses = statuses
        .iter()
        .map(|status| CannedResponse::json(*status, wire::SERVER_BODY))
        .collect();
    let server = MockServer::start(responses, "/v1").await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::RateLimit { retry_after: None }),
        "no five consecutive 429s, so the last failure surfaces: {err:?}"
    );
    assert_eq!(
        server.request_count(),
        6,
        "interleaved failures exhaust the ladder"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn context_length_body_refines_to_context_window() -> Result<(), TestError> {
    fail_fast_row(
        400,
        wire::CONTEXT_BODY,
        |err| matches!(err, ProviderError::ContextWindow { .. }),
        "maximum context length",
    )
    .await
}

#[tokio::test(start_paused = true)]
async fn payload_too_large_fails_fast() -> Result<(), TestError> {
    let server =
        MockServer::start(vec![CannedResponse::json(413, wire::SERVER_BODY)], "/v1").await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(matches!(err, ProviderError::PayloadTooLarge { .. }));
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unknown_model_maps_to_invalid_request() -> Result<(), TestError> {
    fail_fast_row(
        404,
        wire::NOT_FOUND_BODY,
        |err| matches!(err, ProviderError::InvalidRequest { .. }),
        "does not exist",
    )
    .await
}

#[tokio::test(start_paused = true)]
async fn refused_connect_maps_to_transport_after_the_ladder() -> Result<(), TestError> {
    // No fixture binds 127.0.0.2, so no parallel mock can ever hold the port.
    let base_url = "http://127.0.0.2:1/v1";
    let events = run(&provider(base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::Transport { .. }),
        "a dead endpoint surfaces as Transport: {err:?}"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stalled_gateway_surfaces_timeout_and_rides_the_ladder() -> Result<(), TestError> {
    // Sockets are accepted and held, but no response byte is ever
    // written: only the request deadline can end the wait.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = accepted.clone();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::Relaxed);
            held.push(socket);
        }
    });
    let base_url = format!("http://{addr}/v1");
    let provider = provider(&base_url)?.with_request_timeout(Duration::from_millis(50))?;
    let events = run(&provider, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::Timeout { .. }),
        "a stalled gateway must surface Timeout: {err:?}"
    );
    assert!(
        accepted.load(Ordering::Relaxed) >= 2,
        "the timeout row must ride the retry ladder"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn late_client_error_rides_the_patience_window_as_transient() -> Result<(), TestError> {
    // A 400 served only after a long in-flight stall is an upstream
    // artifact: it rides the ladder and the retry meets the canned
    // success, while patience stays open.
    let responses = vec![
        CannedResponse::json(400, wire::NOT_FOUND_BODY).with_delay(Duration::from_secs(130)),
        CannedResponse::json(200, &wire::text_stream()),
    ];
    let server = MockServer::start(responses, "/v1").await?;
    let provider = OpenAiProvider::new(&server.base_url, MODEL, KEY)?
        .with_retry_patience(Duration::from_secs(10_000));
    let events = run(&provider, sample_request()).await;
    assert!(
        events.iter().any(|event| matches!(
            event,
            Ok(ModelEvent::TurnFinished {
                stop_reason: StopReason::EndTurn,
                ..
            })
        )),
        "the retry after the late 400 must succeed: {events:?}"
    );
    assert_eq!(server.request_count(), 2);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn prompt_client_error_fails_fast_without_retry() -> Result<(), TestError> {
    // The same 400 served promptly is a genuine rejection: one request,
    // no ladder.
    let (server, err) = error_over(vec![CannedResponse::json(400, wire::NOT_FOUND_BODY)]).await?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "got {err:?}"
    );
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn transient_5xx_rides_past_the_round_bound_while_patience_lasts() -> Result<(), TestError> {
    // Eight 504s exceed the shared 6-round bound; under an open
    // patience window the ninth request meets the canned success.
    let mut responses: Vec<CannedResponse> = (0..8)
        .map(|_| CannedResponse::json(504, wire::SERVER_BODY))
        .collect();
    responses.push(CannedResponse::json(200, &wire::text_stream()));
    let server = MockServer::start(responses, "/v1").await?;
    let provider = OpenAiProvider::new(&server.base_url, MODEL, KEY)?
        .with_retry_patience(Duration::from_secs(10_000));
    let events = run(&provider, sample_request()).await;
    assert!(
        events.iter().any(|event| matches!(
            event,
            Ok(ModelEvent::TurnFinished {
                stop_reason: StopReason::EndTurn,
                ..
            })
        )),
        "the ninth request must succeed: {events:?}"
    );
    assert_eq!(server.request_count(), 9);
    Ok(())
}
