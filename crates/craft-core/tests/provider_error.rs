//! Provider error taxonomy: closed set of failure kinds, HTTP status
//! mapping, and the Retry-After header parse used by adapters.

use craft_core::error::{ProviderError, parse_retry_after};
use std::time::Duration;

fn status(code: u16) -> reqwest::StatusCode {
    reqwest::StatusCode::from_u16(code).unwrap_or(reqwest::StatusCode::BAD_REQUEST)
}

#[test]
fn unauthorized_and_forbidden_map_to_auth() {
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::UNAUTHORIZED),
        ProviderError::Auth { .. }
    ));
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::FORBIDDEN),
        ProviderError::Auth { .. }
    ));
}

#[test]
fn too_many_requests_maps_to_rate_limit_without_header() {
    let err = ProviderError::from_http_status(reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert!(
        matches!(err, ProviderError::RateLimit { retry_after: None }),
        "got {err:?}"
    );
}

#[test]
fn payload_too_large_maps_from_413() {
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::PAYLOAD_TOO_LARGE),
        ProviderError::PayloadTooLarge { .. }
    ));
}

#[test]
fn client_errors_map_to_invalid_request() {
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::BAD_REQUEST),
        ProviderError::InvalidRequest { .. }
    ));
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::NOT_FOUND),
        ProviderError::InvalidRequest { .. }
    ));
    assert!(matches!(
        ProviderError::from_http_status(status(418)),
        ProviderError::InvalidRequest { .. }
    ));
}

#[test]
fn server_errors_and_request_timeout_map_to_transient() {
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::REQUEST_TIMEOUT),
        ProviderError::Transient { .. }
    ));
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
        ProviderError::Transient { .. }
    ));
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::BAD_GATEWAY),
        ProviderError::Transient { .. }
    ));
    assert!(
        matches!(
            ProviderError::from_http_status(status(529)),
            ProviderError::Transient { .. }
        ),
        "overloaded is in the 5xx family"
    );
}

#[test]
fn non_error_status_in_an_error_context_maps_to_transport() {
    assert!(matches!(
        ProviderError::from_http_status(reqwest::StatusCode::OK),
        ProviderError::Transport { .. }
    ));
}

#[test]
fn invalid_status_code_is_rejected_without_panicking() {
    // The valid range is 100..=999; anything outside must be a typed error.
    assert!(reqwest::StatusCode::from_u16(60000).is_err());
    assert!(reqwest::StatusCode::from_u16(1000).is_err());
    assert!(reqwest::StatusCode::from_u16(99).is_err());
}

#[test]
fn rate_limit_carries_retry_after() {
    let err = ProviderError::RateLimit {
        retry_after: Some(Duration::from_secs(120)),
    };
    assert!(
        matches!(err, ProviderError::RateLimit { retry_after: Some(wait) } if wait == Duration::from_secs(120)),
        "got {err:?}"
    );
}

#[test]
fn retry_after_parses_delta_seconds() {
    assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
    assert_eq!(parse_retry_after("0"), Some(Duration::from_secs(0)));
    assert_eq!(parse_retry_after("  7  "), Some(Duration::from_secs(7)));
}

#[test]
fn retry_after_rejects_hostile_values_without_panicking() {
    assert_eq!(parse_retry_after("garbage"), None);
    assert_eq!(parse_retry_after(""), None);
    assert_eq!(parse_retry_after("-1"), None);
    assert_eq!(parse_retry_after("99999999999999999999999"), None);
    assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), None);
}

#[test]
fn stream_lifecycle_errors_are_part_of_the_closed_set() {
    let cases = vec![
        ProviderError::IdleTimeout,
        ProviderError::StreamClosed,
        ProviderError::EmptyResponse,
        ProviderError::ContextWindow {
            detail: String::from("prompt too long"),
        },
        ProviderError::Transient {
            detail: String::from("overloaded"),
        },
        ProviderError::Transport {
            detail: String::from("connection reset"),
        },
    ];
    for err in cases {
        assert!(!err.to_string().is_empty(), "every variant must display");
    }
    assert_eq!(ProviderError::IdleTimeout, ProviderError::IdleTimeout);
    assert_eq!(
        ProviderError::StreamClosed,
        ProviderError::StreamClosed,
        "unit variants compare for cheap assertion in adapters"
    );
}

#[test]
fn retry_ladder_constants_match_the_plan() {
    assert_eq!(craft_core::error::RETRY_MAX_ATTEMPTS, 6);
    assert_eq!(craft_core::error::RETRY_BACKOFF_INITIAL_SECS, 2);
    assert_eq!(craft_core::error::RETRY_BACKOFF_CAP_SECS, 30);
    assert_eq!(craft_core::error::RATE_LIMIT_STREAK, 5);
}
