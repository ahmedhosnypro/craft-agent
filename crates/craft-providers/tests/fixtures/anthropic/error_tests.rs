//! HTTP error rows: auth fails fast, 429 honors the live Retry-After and
//! surfaces after the streak, 529 and 5xx climb the transient ladder,
//! and error bodies refine the closed taxonomy. Paused time keeps the
//! ladder instant.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{Outcome, TestError, detail_of, provider, run, sample_request};
use craft_core::error::ProviderError;
use std::time::Duration;

fn surfaced_error(events: &[Outcome]) -> Result<ProviderError, TestError> {
    match events.first() {
        Some(Err(err)) => Ok(err.clone()),
        _ => Err(String::from("the row must surface one typed error").into()),
    }
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
    assert!(detail_of(&err).contains("invalid x-api-key"));
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
async fn overloaded_529_maps_transient() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(529, wire::OVERLOADED_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::Transient { .. }),
        "529 overloaded is transient: {err:?}"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn invalid_request_400_maps_invalid_request() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(400, wire::INVALID_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "got {err:?}"
    );
    assert_eq!(server.request_count(), 1, "a 400 never retries");
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
    assert!(matches!(err, ProviderError::Transient { .. }), "{err:?}");
    assert_eq!(server.request_count(), 6, "the ladder maxes at 6 attempts");
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn context_length_body_refines_to_context_window() -> Result<(), TestError> {
    let server = MockServer::start(vec![CannedResponse::json(400, wire::CONTEXT_BODY)]).await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(
        matches!(err, ProviderError::ContextWindow { .. }),
        "got {err:?}"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn refused_connect_maps_to_transport_after_the_ladder() -> Result<(), TestError> {
    // No fixture binds 127.0.0.2, so no parallel mock can ever hold the port.
    let provider = provider("http://127.0.0.2:1")?;
    let events = run(&provider, sample_request()).await;
    let err = surfaced_error(&events)?;
    assert!(matches!(err, ProviderError::Transport { .. }), "{err:?}");
    Ok(())
}
