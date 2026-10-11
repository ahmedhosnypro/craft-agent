use craft_digest::DigestError;
use craft_digest::window::{
    MAX_WINDOW_DAYS, WindowSource, derive_since, marker, parse_marker, resolve_window,
};

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
fn derive_since_uses_last_marker_end() {
    let body = "digest prose\n\n<!-- digest-window: 2026-10-09..2026-10-10 -->\n";
    let w = derive_since(body, "2026-10-11").unwrap();
    assert_eq!(w.since, "2026-10-10");
    assert_eq!(w.until, "2026-10-11");
    assert!(matches!(w.source, WindowSource::Marker));
}

#[test]
fn derive_since_last_marker_wins() {
    let body = "<!-- digest-window: 2026-10-01..2026-10-02 -->\nmid prose\n<!-- digest-window: 2026-10-09..2026-10-10 -->\n";
    let w = derive_since(body, "2026-10-11").unwrap();
    assert_eq!(w.since, "2026-10-10");
}

#[test]
fn derive_since_absent_marker_falls_back() {
    let w = derive_since("no marker here", "2026-10-11").unwrap();
    assert_eq!(w.since, "2026-10-10");
    assert!(matches!(w.source, WindowSource::Fallback));
}

#[test]
fn derive_since_hostile_markers_fall_back() {
    for body in [
        "<!-- digest-window: junk..2026-10-10 -->",
        "<!-- digest-window: 2026-10-10..2026-10-09 -->",
        "<!-- digest-window: 2026-10-10 -->",
        "<!-- digest-window: 2023-02-29..2023-03-01 -->",
        "<!-- digest-window: .. -->",
    ] {
        let w = derive_since(body, "2026-10-11").unwrap();
        assert_eq!(w.since, "2026-10-10", "accepted {body:?}");
        assert!(matches!(w.source, WindowSource::Fallback), "src {body:?}");
    }
}

#[test]
fn derive_since_clamps_stale_marker_to_thirty_days() {
    let body = "<!-- digest-window: 2026-08-01..2026-08-02 -->";
    let w = derive_since(body, "2026-10-11").unwrap();
    assert_eq!(w.since, "2026-09-11");
    assert!(matches!(w.source, WindowSource::Marker));
}

#[test]
fn marker_round_trips_through_parse() {
    let body = "prose\n<!-- digest-window: 2026-10-10..2026-10-11 -->\n";
    let w = parse_marker(body).unwrap();
    assert_eq!(w.since, "2026-10-10");
    assert_eq!(w.until, "2026-10-11");
    assert_eq!(marker(&w), "<!-- digest-window: 2026-10-10..2026-10-11 -->");
    assert_eq!(
        derive_since(body, "2026-10-11").unwrap().since,
        "2026-10-11"
    );
}

#[test]
fn parse_marker_without_marker_is_typed_error() {
    assert!(matches!(
        parse_marker("nothing").unwrap_err(),
        DigestError::Window(_)
    ));
}

#[test]
fn leap_day_is_valid_only_in_leap_years() {
    let w = derive_since(
        "<!-- digest-window: 2024-02-28..2024-02-29 -->",
        "2026-10-11",
    )
    .unwrap();
    assert!(matches!(w.source, WindowSource::Marker));
    assert_eq!(w.since, "2026-09-11"); // stale marker, clamped
    let w = derive_since(
        "<!-- digest-window: 2023-02-28..2023-02-29 -->",
        "2026-10-11",
    )
    .unwrap();
    assert!(matches!(w.source, WindowSource::Fallback));
}
