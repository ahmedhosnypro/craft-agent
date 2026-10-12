//! glob tool: bounded, sorted, deterministic results; skip rules for .git
//! and sensitive files; typed errors for hostile patterns.

mod common;

use common::{assert_clean, ctx_for, error, fast_limits, run, terminal, tmp_ws, write};
use craft_tools::{Registry, ToolError};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

fn lines(out: &str) -> Vec<&str> {
    out.lines().filter(|l| !l.starts_with('[')).collect()
}

#[tokio::test]
async fn lists_matching_files_sorted_and_relative() {
    let ws = tmp_ws("glob-basic");
    write(&ws, "b.txt", "1");
    write(&ws, "a.txt", "1");
    write(&ws, "c.md", "1");
    write(&ws, "sub/d.txt", "1");
    let events = run(reg().execute("glob", json!({ "pattern": "*.txt" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    assert_eq!(lines(&terminal(&events)), vec!["a.txt", "b.txt"]);
}

#[tokio::test]
async fn double_star_recurses() {
    let ws = tmp_ws("glob-recursive");
    write(&ws, "top.rs", "1");
    write(&ws, "sub/inner/deep.rs", "1");
    write(&ws, "sub/other.txt", "1");
    let events = run(reg().execute("glob", json!({ "pattern": "**/*.rs" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    let out = terminal(&events);
    let mut got = lines(&out).to_vec();
    got.sort_unstable();
    assert_eq!(got, vec!["sub/inner/deep.rs", "top.rs"]);
}

#[tokio::test]
async fn results_across_sibling_dirs_are_sorted() {
    let ws = tmp_ws("glob-sibling-order");
    write(&ws, "a-dir/x.txt", "1");
    write(&ws, "b-dir/y.txt", "1");
    let events = run(reg().execute("glob", json!({ "pattern": "**/*.txt" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    assert_eq!(
        lines(&terminal(&events)),
        vec!["a-dir/x.txt", "b-dir/y.txt"]
    );
}

#[tokio::test]
async fn file_target_matches_by_name() {
    let ws = tmp_ws("glob-file-target");
    write(&ws, "sub/x.txt", "1");
    let events = run(reg().execute(
        "glob",
        json!({ "pattern": "*.txt", "path": "sub/x.txt" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "sub/x.txt\n");
}

#[tokio::test]
async fn results_are_bounded_with_a_note() {
    let ws = tmp_ws("glob-capped");
    for i in 0..8 {
        write(&ws, &format!("f{i}.txt"), "1");
    }
    let events = run(reg().execute("glob", json!({ "pattern": "*.txt" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert_eq!(lines(&out).len(), 6, "cap is 6: {out}");
    assert!(out.contains("["), "capping note expected: {out}");
}

#[tokio::test]
async fn git_dir_and_sensitive_files_are_never_listed() {
    let ws = tmp_ws("glob-skips");
    write(&ws, ".git/config", "1");
    write(&ws, ".env", "SECRET=1\n");
    write(&ws, ".env.example", "SAFE=1\n");
    write(&ws, "keep.txt", "1");
    let events = run(reg().execute("glob", json!({ "pattern": "**" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(
        out.contains(".env.example") && out.contains("keep.txt"),
        "{out}"
    );
    assert!(!out.contains(".git/") && !out.contains(".env\n"), "{out}");
}

#[tokio::test]
async fn scoped_path_must_stay_inside_the_workspace() {
    let ws = tmp_ws("glob-scope");
    write(&ws, "sub/x.txt", "1");
    let events = run(reg().execute(
        "glob",
        json!({ "pattern": "*.txt", "path": ".." }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::EscapesWorkspace { .. }));

    let events = run(reg().execute(
        "glob",
        json!({ "pattern": "*.txt", "path": "sub" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "sub/x.txt\n");
}

#[tokio::test]
async fn hostile_patterns_and_args_are_typed_errors() {
    let ws = tmp_ws("glob-args");
    for args in [
        json!({}),
        json!({ "pattern": "[[" }),
        json!({ "pattern": "" }),
        json!({ "pattern": "x*.txt", "unknown": 1 }),
    ] {
        let shown = args.to_string();
        let events = run(reg().execute("glob", args, &ctx_for(&ws))).await;
        assert!(
            matches!(error(&events), ToolError::InvalidArgs { .. }),
            "{shown}"
        );
    }
    let _ = fs::read_dir(&ws).map(|_| ());
}
