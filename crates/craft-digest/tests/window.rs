use craft_digest::DigestError;
use craft_digest::window::{MAX_WINDOW_DAYS, WindowSource, marker, resolve_window};

#[test]
fn resolve_defaults_to_last_twenty_four_hours() {
    let w = resolve_window("2026-10-11", None).unwrap();
    assert_eq!(w.since, "2026-10-10");
    assert_eq!(w.until, "2026-10-11");
    assert!(matches!(w.source, WindowSource::Fallback));
}

#[test]
fn resolve_accepts_explicit_since() {
    let w = resolve_window("2026-10-11", Some("2026-10-05")).unwrap();
    assert_eq!(w.since, "2026-10-05");
    assert_eq!(w.until, "2026-10-11");
    assert!(matches!(w.source, WindowSource::Marker));
}

#[test]
fn resolve_clamps_since_to_max_window() {
    let w = resolve_window("2026-10-11", Some("2026-01-01")).unwrap();
    assert_eq!(w.since, "2026-09-11");
    assert_eq!(w.until, "2026-10-11");
    assert_eq!(MAX_WINDOW_DAYS, 30);
}

#[test]
fn resolve_clamps_future_since_to_today() {
    let w = resolve_window("2026-10-11", Some("2026-12-01")).unwrap();
    assert_eq!(w.since, "2026-10-11");
    assert_eq!(w.until, "2026-10-11");
}

#[test]
fn resolve_rejects_malformed_since() {
    for bad in ["2026-13-01", "2026-02-30", "junk", "2026-10-1", ""] {
        let err = resolve_window("2026-10-11", Some(bad)).unwrap_err();
        assert!(matches!(err, DigestError::Window(_)), "accepted {bad:?}");
    }
}

#[test]
fn year_zero_dates_are_rejected() {
    for bad in ["0000-01-01", "0000-02-29", "0000-12-31"] {
        let err = resolve_window("2026-10-11", Some(bad)).unwrap_err();
        assert!(matches!(err, DigestError::Window(_)), "accepted {bad:?}");
    }
}

#[test]
fn year_one_january_boundary_math_is_exact() {
    let w = resolve_window("0001-01-05", None).unwrap();
    assert_eq!(w.since, "0001-01-04");
    assert_eq!(w.until, "0001-01-05");
    let w = resolve_window("0001-01-05", Some("0001-01-01")).unwrap();
    assert_eq!(w.since, "0001-01-01");
}

#[test]
fn marker_renders_the_window_range() {
    let w = resolve_window("2026-10-11", Some("2026-10-10")).unwrap();
    assert_eq!(marker(&w), "<!-- digest-window: 2026-10-10..2026-10-11 -->");
}

#[test]
fn leap_day_is_valid_only_in_leap_years() {
    let w = resolve_window("2024-03-01", Some("2024-02-29")).unwrap();
    assert_eq!(w.since, "2024-02-29");
    let err = resolve_window("2024-03-01", Some("2023-02-29")).unwrap_err();
    assert!(matches!(err, DigestError::Window(_)));
}
