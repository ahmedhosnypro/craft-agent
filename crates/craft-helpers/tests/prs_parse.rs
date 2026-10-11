// prs corpus: hostile parsing of recorded REST rows.
#![allow(clippy::unwrap_used)]

mod common;

use common::fixture;
use craft_helpers::error::HelperError;
use craft_helpers::prs::{MAX_PULL_ROWS, parse_file_rows, parse_pull_detail, parse_pull_page};
use craft_helpers::prs_run::resolve_token;
use craft_helpers::time::parse_rfc3339_secs;

#[test]
fn recorded_page_parses_into_rows() {
    let rows = parse_pull_page(&fixture("pulls_page1.json")).unwrap();
    let numbers: Vec<u64> = rows.iter().map(|row| row.number).collect();
    assert_eq!(numbers, vec![101, 100, 99, 98]);
    let first = &rows[0];
    assert_eq!(first.title, "Fix the digest window clamp");
    assert_eq!(
        first.url,
        "https://github.com/storytold/craft-agent/pull/101"
    );
    assert_eq!(first.author, "dev1");
    assert_eq!(first.merged_at, "2026-10-11T08:00:00Z");
    assert_eq!(rows[2].merged_at, "");
}

#[test]
fn malformed_bodies_are_typed_errors() {
    for body in ["{\"broken\":", "[] trailing", "42"] {
        let err = parse_pull_page(body).unwrap_err();
        assert!(matches!(err, HelperError::MalformedJson(_)), "{body}");
    }
    let object_err = parse_pull_page("{\"not\":\"an array\"}").unwrap_err();
    assert!(matches!(object_err, HelperError::MalformedJson(_)));
}

#[test]
fn page_rows_are_capped() {
    let mut body = String::from('[');
    for number in 0..(MAX_PULL_ROWS * 3 / 2) {
        let row = format!(
            "{{\"number\":{number},\"title\":\"t\",\"html_url\":\"u\",\"user\":{{\"login\":\"a\"}},\"merged_at\":\"2026-10-11T00:00:00Z\"}}"
        );
        body.push_str(&row);
        body.push(',');
    }
    body.pop();
    body.push(']');
    let rows = parse_pull_page(&body).unwrap();
    assert_eq!(rows.len(), MAX_PULL_ROWS);
}

#[test]
fn pull_detail_reads_totals() {
    let detail = parse_pull_detail(&fixture("pull_101.json")).unwrap();
    assert_eq!(detail.additions, 120);
    assert_eq!(detail.deletions, 30);
    let minimal = parse_pull_detail("{\"additions\":1}").unwrap();
    assert_eq!(minimal.additions, 1);
    assert_eq!(minimal.deletions, 0);
    for body in ["[]", "{\"broken\"", "\"x\""] {
        assert!(matches!(
            parse_pull_detail(body).unwrap_err(),
            HelperError::MalformedJson(_)
        ));
    }
}

#[test]
fn file_rows_map_filename_to_path() {
    let rows = parse_file_rows(&fixture("files_101.json")).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].path, "crates/craft-digest/src/window.rs");
    assert_eq!(rows[0].additions, 42);
    assert_eq!(rows[0].deletions, 7);
    assert_eq!(rows[0].status, "modified");
    assert_eq!(rows[2].status, "modified");
    let err = parse_file_rows("{\"broken\":").unwrap_err();
    assert!(matches!(err, HelperError::MalformedJson(_)));
}

#[test]
fn file_rows_are_capped() {
    let mut body = String::from('[');
    for index in 0..250 {
        let row = format!(
            "{{\"filename\":\"f{index}\",\"status\":\"added\",\"additions\":1,\"deletions\":0}}"
        );
        body.push_str(&row);
        body.push(',');
    }
    body.pop();
    body.push(']');
    let rows = parse_file_rows(&body).unwrap();
    assert_eq!(rows.len(), 100);
}

#[test]
fn rfc3339_windows_parse_strictly() {
    assert_eq!(
        parse_rfc3339_secs("2026-10-11T08:00:00Z"),
        Some(1_791_705_600)
    );
    assert_eq!(
        parse_rfc3339_secs("2026-10-11T10:00:00+02:00"),
        Some(1_791_705_600)
    );
    assert_eq!(
        parse_rfc3339_secs("2024-02-29T00:00:00Z"),
        Some(1_709_164_800)
    );
    assert_eq!(parse_rfc3339_secs(""), None);
    assert_eq!(parse_rfc3339_secs("2026-13-11T08:00:00Z"), None);
    assert_eq!(parse_rfc3339_secs("2026-10-32T08:00:00Z"), None);
    assert_eq!(parse_rfc3339_secs("2026-02-30T08:00:00Z"), None);
    assert_eq!(parse_rfc3339_secs("2026-10-11T24:00:00Z"), None);
    assert_eq!(parse_rfc3339_secs("2026-10-11T08:60:00Z"), None);
    assert_eq!(parse_rfc3339_secs("2026-10-11 08:00:00Z"), None);
    assert_eq!(parse_rfc3339_secs("2026-10-11T08:00:00"), None);
}

#[test]
fn token_resolution_names_the_env_var() {
    let missing = resolve_token(|_| None).unwrap_err();
    assert!(matches!(missing, HelperError::MissingToken));
    assert!(missing.to_string().contains("GH_TOKEN"));
    let blank = resolve_token(|_| Some("   ".to_string())).unwrap_err();
    assert!(matches!(blank, HelperError::MissingToken));
    let token = resolve_token(|_| Some(" tok ".to_string())).unwrap();
    assert_eq!(token, "tok");
}
