//! The step-retry knob corpus: `CRAFT_AGENT_STEP_RETRIES` follows the
//! transport-knob parse idiom — unset or empty keeps the default, zero
//! restores fail-fast, and a hostile value is a typed error naming the
//! knob. Pure rows; no provider doubles.

use craft_core::error::ProviderError;
use craft_runtime::retry::{DEFAULT_STEP_ATTEMPTS, STEP_RETRIES_ENV, step_attempts};

#[test]
fn the_knob_rows_match_the_transport_parse_idiom() {
    assert_eq!(STEP_RETRIES_ENV, "CRAFT_AGENT_STEP_RETRIES");
    assert_eq!(DEFAULT_STEP_ATTEMPTS, 3);
    assert_eq!(step_attempts(None), Ok(3));
    assert_eq!(step_attempts(Some("")), Ok(3));
    assert_eq!(step_attempts(Some("5")), Ok(5));
    assert_eq!(step_attempts(Some(" 4 ")), Ok(4));
    assert_eq!(
        step_attempts(Some("0")),
        Ok(1),
        "zero restores fail-fast: a single attempt"
    );
    assert_eq!(step_attempts(Some("1")), Ok(1));
}

#[test]
fn hostile_knob_values_surface_typed_errors_naming_the_knob() {
    for raw in ["garbage", "-5", "1.5", "1e3", "99999999999999999999"] {
        let err = step_attempts(Some(raw))
            .err()
            .unwrap_or_else(|| panic!("a hostile value must fail, not fall back: {raw}"));
        assert!(
            matches!(err, ProviderError::InvalidRequest { .. }),
            "got {err:?}"
        );
        assert!(err.to_string().contains(STEP_RETRIES_ENV), "{err}");
        assert!(err.to_string().contains(raw), "{err}");
    }
}

#[test]
fn the_patience_rows_match_the_transport_parse_idiom() {
    use std::time::Duration;
    assert_eq!(
        craft_core::error::retry_patience(None),
        Ok(Duration::from_secs(
            craft_core::error::RETRY_PATIENCE_DEFAULT.as_secs()
        ))
    );
    assert_eq!(
        craft_core::error::retry_patience(Some("")),
        Ok(craft_core::error::RETRY_PATIENCE_DEFAULT)
    );
    assert_eq!(
        craft_core::error::retry_patience(Some(" 90 ")),
        Ok(Duration::from_secs(90))
    );
    assert_eq!(
        craft_core::error::retry_patience(Some("0")),
        Ok(Duration::ZERO),
        "zero disables: the attempt bound binds alone"
    );
    for raw in ["garbage", "-5", "1.5"] {
        let err = craft_core::error::retry_patience(Some(raw))
            .err()
            .unwrap_or_else(|| panic!("a hostile value must fail, not fall back: {raw}"));
        assert!(err.contains(craft_core::error::RETRY_PATIENCE_ENV), "{err}");
        assert!(err.contains(raw), "{err}");
    }
}
