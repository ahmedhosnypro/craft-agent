//! The transport deadline knob: hostile values surface as typed errors
//! naming the knob, unset or empty keeps the default, and the constants
//! match the seam contract.

use std::time::Duration;

use craft_core::error::ProviderError;
use craft_providers::transport::{
    CONNECT_TIMEOUT_SECS, DEFAULT_REQUEST_TIMEOUT_SECS, REQUEST_TIMEOUT_ENV, request_timeout,
};

type TestError = Box<dyn std::error::Error + Send + Sync>;

#[test]
fn unset_value_keeps_the_default_deadline() -> Result<(), TestError> {
    assert_eq!(
        request_timeout(None),
        Ok(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
    );
    assert_eq!(DEFAULT_REQUEST_TIMEOUT_SECS, 300);
    Ok(())
}

#[test]
fn empty_value_keeps_the_default_deadline() -> Result<(), TestError> {
    assert_eq!(
        request_timeout(Some("")),
        Ok(Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
    );
    Ok(())
}

#[test]
fn a_valid_value_is_honored() -> Result<(), TestError> {
    assert_eq!(request_timeout(Some("900")), Ok(Duration::from_secs(900)));
    assert_eq!(request_timeout(Some(" 120 ")), Ok(Duration::from_secs(120)));
    Ok(())
}

#[test]
fn hostile_values_surface_typed_errors_naming_the_knob() -> Result<(), TestError> {
    for raw in [
        "garbage",
        "12.5",
        "-5",
        "1e3",
        "0",
        "99999999999999999999999",
    ] {
        let err = request_timeout(Some(raw))
            .err()
            .ok_or("a hostile value must fail, not fall back")?;
        assert!(
            matches!(err, ProviderError::InvalidRequest { .. }),
            "got {err:?}"
        );
        assert!(err.to_string().contains(REQUEST_TIMEOUT_ENV), "{err}");
        assert!(err.to_string().contains(raw), "{err}");
    }
    Ok(())
}

#[test]
fn the_seam_contract_constants_hold() -> Result<(), TestError> {
    assert_eq!(REQUEST_TIMEOUT_ENV, "CRAFT_AGENT_REQUEST_TIMEOUT_SECS");
    assert_eq!(CONNECT_TIMEOUT_SECS, 15);
    Ok(())
}
