//! HTTP error rows: the 400 family refines by status string, auth fails
//! fast, 429 honors the live Retry-After and surfaces after the streak,
//! 5xx climbs the ladder. Paused time keeps the ladder instant.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{Outcome, TestError, detail_of, provider, run, sample_request};
use craft_core::error::ProviderError;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn surfaced_error(events: &[Outcome]) -> Result<ProviderError, TestError> {
    match events.first() {
        Some(Err(err)) => Ok(err.clone()),
        _ => Err(String::from("the row must surface one typed error").into()),
    }
}

#[tokio::test(start_paused = true)]
async fn invalid_argument_400_maps_to_invalid_request() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(400, wire::INVALID_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "400 must map to InvalidRequest: {err:?}"
    );
    assert!(detail_of(&err).contains("Invalid value"));
    assert_eq!(server.request_count(), 1, "client errors never retry");
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn the_api_key_missing_shape_is_a_400_rejection() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(400, wire::KEY_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "the documented bad-key shape arrives as a 400, so it stays InvalidRequest: {err:?}"
    );
    assert!(detail_of(&err).contains("API key not valid"));
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn auth_401_fails_fast_without_retry() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(401, wire::AUTH_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::Auth { .. }),
        "401 must map to Auth: {err:?}"
    );
    assert!(detail_of(&err).contains("invalid authentication credentials"));
    assert_eq!(server.request_count(), 1, "auth failures never retry");
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn auth_403_fails_fast_without_retry() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(403, wire::FORBIDDEN_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(matches!(err, ProviderError::Auth { .. }), "got {err:?}");
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn rate_limit_streak_honors_the_live_retry_after() -> Result<(), TestError> {
    let responses = (0..5)
        .map(|_| CannedResponse::json(429, wire::RATE_BODY).with_header("retry-after", "3"))
        .collect();
    let server = MockServer::start(responses).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
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
    let server = MockServer::start(responses).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert_eq!(err, ProviderError::RateLimit { retry_after: None });
    assert_eq!(server.request_count(), 5);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn server_errors_climb_the_full_ladder() -> Result<(), TestError> {
    let responses = (0..6)
        .map(|_| CannedResponse::json(500, wire::SERVER_BODY))
        .collect();
    let server = MockServer::start(responses).await?;
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
async fn context_window_body_refines_to_context_window() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(400, wire::CONTEXT_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::ContextWindow { .. }),
        "the token-count message must refine the 400: {err:?}"
    );
    assert!(detail_of(&err).contains("context window"));
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn payload_too_large_fails_fast() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(413, wire::SERVER_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(matches!(err, ProviderError::PayloadTooLarge { .. }));
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn unknown_model_maps_to_invalid_request() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(404, wire::NOT_FOUND_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "{err:?}"
    );
    assert!(detail_of(&err).contains("does not exist"));
    assert_eq!(server.request_count(), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn refused_connect_maps_to_transport_after_the_ladder() -> Result<(), TestError> {
    // No fixture binds 127.0.0.2, so no parallel mock can ever hold the port.
    let base_url = "http://127.0.0.2:1";
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
    let base_url = format!("http://{addr}");
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
