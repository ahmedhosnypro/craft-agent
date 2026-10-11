use std::path::{Path, PathBuf};

use craft_digest::DigestError;
use craft_digest::jscpd::{
    JSCPD_REPORT_CAP_BYTES, MAX_DUPLICATE_CANDIDATES, parse_jscpd_report, read_jscpd_report,
};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jscpd-report.json")
}

#[test]
fn recorded_report_parses_and_sorts_by_tokens() {
    let candidates = read_jscpd_report(&fixture_path()).unwrap();
    let tokens: Vec<u64> = candidates.iter().map(|c| c.tokens).collect();
    assert_eq!(tokens, vec![105, 79, 73]);
    let first = &candidates[0];
    assert_eq!(first.first.repo, "app-a");
    assert_eq!(first.first.file, "src/lib.rs");
    assert_eq!(first.first.line, 3);
    assert_eq!(first.second.repo, "app-b");
    assert_eq!(first.second.file, "src/lib.rs");
    assert_eq!(first.second.line, 2);
    assert_eq!(first.lines, 15);
}

#[test]
fn synthetic_pairs_are_capped_and_sorted_desc() {
    let entries: Vec<String> = (0..25)
        .map(|i| {
            let tokens = (i * 7) % 25 + 1;
            format!(
                r#"{{"firstFile":{{"name":"r{i}/a.rs","start":{i}}},"secondFile":{{"name":"r{i}/b.rs","start":{i}}},"tokens":{tokens},"lines":5}}"#
            )
        })
        .collect();
    let json = format!(r#"{{"duplicates":[{}]}}"#, entries.join(","));
    let candidates = parse_jscpd_report(&json).unwrap();
    assert_eq!(candidates.len(), MAX_DUPLICATE_CANDIDATES);
    assert_eq!(MAX_DUPLICATE_CANDIDATES, 20);
    let tokens: Vec<u64> = candidates.iter().map(|c| c.tokens).collect();
    assert_eq!(tokens, (6..=25).rev().collect::<Vec<u64>>());
}

#[test]
fn garbage_reports_are_typed_errors() {
    for bad in ["not json", "{}", "[]", r#"{"duplicates":null}"#, ""] {
        let err = parse_jscpd_report(bad).unwrap_err();
        assert!(matches!(err, DigestError::Jscpd(_)), "took {bad:?}");
    }
}

#[test]
fn malformed_entries_are_skipped_not_fatal() {
    let json = r#"{"duplicates":[
 {"firstFile":{"name":"app-a/src/lib.rs","start":1},"secondFile":{"name":"app-b/src/lib.rs","start":2},"tokens":50},
 {"firstFile":{"start":1},"secondFile":{"name":"app-b/x.rs","start":2},"tokens":60},
 {"firstFile":{"name":"no-slash.rs","start":1},"secondFile":{"name":"app-b/x.rs","start":2},"tokens":70},
 {"firstFile":{"name":"app-a/y.rs","start":1},"secondFile":{"name":"app-b/x.rs","start":2}},
 {"firstFile":{"name":"app-a/y.rs","start":1},"secondFile":{"name":"app-b/x.rs","start":2},"tokens":"lots"},
 {"firstFile":{"name":"app-a/y.rs","start":1},"secondFile":{"name":"app-b/x.rs","start":2},"tokens":90}
]}"#;
    let candidates = parse_jscpd_report(json).unwrap();
    let tokens: Vec<u64> = candidates.iter().map(|c| c.tokens).collect();
    assert_eq!(tokens, vec![90, 50]);
}

#[test]
fn oversized_report_is_a_typed_error() {
    let huge = "x".repeat(JSCPD_REPORT_CAP_BYTES + 1);
    assert!(matches!(
        parse_jscpd_report(&huge).unwrap_err(),
        DigestError::JscpdTooLarge(_)
    ));
}

#[test]
fn missing_report_file_is_a_typed_error() {
    let err = read_jscpd_report(Path::new("/nonexistent/jscpd-report.json")).unwrap_err();
    assert!(matches!(err, DigestError::Jscpd(_)));
}
