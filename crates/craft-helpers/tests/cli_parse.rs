// cli corpus: bounded argv parsing for the look-back window.
use craft_helpers::cli::{Lookback, parse_lookback};
use craft_helpers::error::{HelperError, MESSAGE_CHARS};

#[test]
fn lookback_accepts_bounded_units() {
    assert_eq!(parse_lookback("366d").unwrap().secs, 366 * 24 * 3600);
    assert_eq!(parse_lookback("90m").unwrap().secs, 5_400);
    assert_eq!(parse_lookback("2h").unwrap().secs, 7_200);
    assert_eq!(parse_lookback("45s").unwrap().secs, 45);
    let Lookback { label, .. } = parse_lookback("2h").unwrap();
    assert_eq!(label, "2h");
}

#[test]
fn lookback_rejects_hostile_windows() {
    for raw in ["", "d", "10x", "999999d", "367d", "366d extra", "-5d"] {
        assert!(parse_lookback(raw).is_err(), "{raw}");
    }
}

#[test]
fn invalid_window_error_is_capped() {
    let raw = "9".repeat(MESSAGE_CHARS * 4) + "x";
    let Err(HelperError::InvalidWindow(value)) = parse_lookback(&raw) else {
        panic!("expected InvalidWindow for a hostile window");
    };
    assert!(value.chars().count() <= MESSAGE_CHARS);
}
