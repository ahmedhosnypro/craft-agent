//! grep tool: Rust-native content search. Literal vs regex modes, bounded
//! results and line excerpts, binary and oversize skipping, invalid
//! patterns, sensitive skips, scoped targets.

mod common;

use common::{
    assert_clean, ctx_for, error, fast_limits, run, terminal, tmp_ws, write, write_bytes,
};
use craft_tools::{Registry, ToolError};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

fn matches(out: &str) -> Vec<&str> {
    out.lines().filter(|l| !l.starts_with('[')).collect()
}

#[tokio::test]
async fn literal_mode_matches_exact_text_not_regex() {
    let ws = tmp_ws("grep-literal");
    write(&ws, "a.txt", "axb\na.b\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "a.b", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert_eq!(matches(&out), vec!["a.txt:2: a.b"], "{out}");

    let events = run(reg().execute("grep", json!({ "pattern": "a.b" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    assert_eq!(
        matches(&terminal(&events)).len(),
        2,
        "regex mode treats . as wildcard"
    );
}

#[tokio::test]
async fn regex_mode_compiles_and_matches() {
    let ws = tmp_ws("grep-regex");
    write(&ws, "words.txt", "color\ncolour\ncolourful\n");
    let events = run(reg().execute("grep", json!({ "pattern": "colou?r$" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert_eq!(
        matches(&out),
        vec!["words.txt:1: color", "words.txt:2: colour"],
        "{out}"
    );
}

#[tokio::test]
async fn results_are_bounded_with_a_note() {
    let ws = tmp_ws("grep-capped");
    let mut body = String::new();
    for i in 0..10 {
        body.push_str(&format!("hit {i}\n"));
    }
    write(&ws, "many.txt", &body);
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "hit", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert_eq!(matches(&out).len(), 5, "cap is 5: {out}");
    assert!(out.contains("["), "capping note expected: {out}");
}

#[tokio::test]
async fn long_lines_are_excerpted_to_a_bound() {
    let ws = tmp_ws("grep-longline");
    let long = format!("{}needle{}", "p".repeat(500), "q".repeat(500));
    write(&ws, "long.txt", &long);
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    let hit = matches(&out).first().expect("one match").to_string();
    assert!(hit.len() <= 64 + 32, "excerpt must be bounded: {hit}");
    assert!(hit.contains("needle"), "{hit}");
}

#[tokio::test]
async fn binary_and_oversize_files_are_skipped_with_notes() {
    let ws = tmp_ws("grep-skip");
    write_bytes(&ws, "blob.bin", b"needle\x00binary");
    let big = format!("{}needle", "z".repeat(2000));
    write(&ws, "big.txt", &big);
    write(&ws, "small.txt", "needle found\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert_eq!(matches(&out), vec!["small.txt:1: needle found"], "{out}");
    assert!(out.contains("["), "skip notes expected: {out}");
}

#[tokio::test]
async fn invalid_regex_is_a_typed_error_with_a_message() {
    let ws = tmp_ws("grep-badpattern");
    let events = run(reg().execute("grep", json!({ "pattern": "[" }), &ctx_for(&ws))).await;
    assert!(matches!(error(&events), ToolError::BadPattern { .. }));
}

#[tokio::test]
async fn explicit_sensitive_file_target_hard_errors() {
    let ws = tmp_ws("grep-sensitive-target");
    write(&ws, ".env", "SECRET=1\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "SECRET", "path": ".env", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::SensitivePath { .. }));
}

#[tokio::test]
async fn scoped_directory_search_and_outside_scope_rejection() {
    let ws = tmp_ws("grep-scope");
    write(&ws, "sub/x.txt", "needle here\n");
    write(&ws, "top.txt", "needle elsewhere\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "path": "sub", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(
        matches(&terminal(&events)),
        vec!["sub/x.txt:1: needle here"]
    );

    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "path": "..", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::EscapesWorkspace { .. }));
}

#[tokio::test]
async fn a_single_file_target_is_searched_directly() {
    let ws = tmp_ws("grep-file");
    write(&ws, "one.txt", "alpha\nneedle\nomega\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "path": "one.txt", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(matches(&terminal(&events)), vec!["one.txt:2: needle"]);
}

#[tokio::test]
async fn no_matches_is_a_clean_terminal() {
    let ws = tmp_ws("grep-none");
    write(&ws, "one.txt", "nothing relevant\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "zebra", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert!(
        terminal(&events).contains("no matches"),
        "explain the empty result"
    );
}

#[tokio::test]
async fn hostile_args_are_typed_errors() {
    let ws = tmp_ws("grep-args");
    for args in [
        json!({}),
        json!({ "pattern": "x".repeat(300) }),
        json!({ "pattern": "x", "literal": "yes" }),
    ] {
        let shown = args.to_string();
        let events = run(reg().execute("grep", args, &ctx_for(&ws))).await;
        assert!(
            matches!(error(&events), ToolError::InvalidArgs { .. }),
            "{shown}"
        );
    }
    let _ = fs::read_dir(&ws).map(|_| ());
}

#[tokio::test]
async fn glob_filter_narrows_the_search() {
    let ws = tmp_ws("grep-filter");
    write(&ws, "keep.rs", "needle\n");
    write(&ws, "skip.txt", "needle\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "literal": true, "glob": "*.rs" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(matches(&terminal(&events)), vec!["keep.rs:1: needle"]);
}

#[tokio::test]
async fn glob_filter_matches_a_single_file_target_by_name() {
    let ws = tmp_ws("grep-file-glob");
    write(&ws, "one.rs", "needle\n");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "path": "one.rs", "literal": true, "glob": "*.rs" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(matches(&terminal(&events)), vec!["one.rs:1: needle"]);
}

#[tokio::test]
async fn rows_are_sorted_by_path_then_line_number() {
    let ws = tmp_ws("grep-row-order");
    let mut body = String::new();
    for i in 1..=12 {
        body.push_str(&format!("needle {i}\n"));
    }
    write(&ws, "b/m.txt", &body);
    write(&ws, "a/m.txt", "needle here\n");
    let mut limits = fast_limits();
    limits.grep_max_results = 20;
    let events = run(Registry::with_builtins(limits).execute(
        "grep",
        json!({ "pattern": "needle", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let mut expected = vec!["a/m.txt:1: needle here".to_string()];
    for i in 1..=12 {
        expected.push(format!("b/m.txt:{i}: needle {i}"));
    }
    let got: Vec<String> = matches(&terminal(&events))
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(got, expected);
}

#[tokio::test]
async fn unreadable_file_is_reported_as_unreadable_not_binary() {
    use std::os::unix::fs::PermissionsExt;
    let ws = tmp_ws("grep-unreadable");
    write(&ws, "locked.txt", "needle\n");
    write(&ws, "open.txt", "needle\n");
    let locked = ws.join("locked.txt");
    let mut perms = fs::metadata(&locked).expect("fixture meta").permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&locked, perms).expect("fixture chmod");
    let events = run(reg().execute(
        "grep",
        json!({ "pattern": "needle", "literal": true }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert_eq!(matches(&out), vec!["open.txt:1: needle"], "{out}");
    assert!(out.contains("[skipped 1 unreadable file"), "{out}");
    assert!(!out.contains("binary"), "{out}");
}
