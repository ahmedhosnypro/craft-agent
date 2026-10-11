//! read_file: exact content, size guard, missing targets, lossy non-UTF8.

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

#[tokio::test]
async fn reads_exact_file_content() {
    let ws = tmp_ws("read-ok");
    write(&ws, "notes.md", "line one\nline two\n");
    let events =
        run(reg().execute("read_file", json!({ "path": "notes.md" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "line one\nline two\n");
}

#[tokio::test]
async fn absolute_paths_inside_the_workspace_work() {
    let ws = tmp_ws("read-abs");
    write(&ws, "a.txt", "content\n");
    let abs = ws.join("a.txt").display().to_string();
    let events = run(reg().execute("read_file", json!({ "path": abs }), &ctx_for(&ws))).await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "content\n");
}

#[tokio::test]
async fn missing_file_is_a_typed_not_found_error() {
    let ws = tmp_ws("read-missing");
    let events =
        run(reg().execute("read_file", json!({ "path": "gone.txt" }), &ctx_for(&ws))).await;
    assert!(matches!(error(&events), ToolError::NotFound { .. }));
}

#[tokio::test]
async fn directory_target_is_a_typed_error() {
    let ws = tmp_ws("read-dir");
    let events = run(reg().execute("read_file", json!({ "path": "." }), &ctx_for(&ws))).await;
    assert!(matches!(error(&events), ToolError::Io { .. }), "{events:?}");
}

#[tokio::test]
async fn oversized_file_hits_the_size_guard() {
    let ws = tmp_ws("read-large");
    let big = "x".repeat(2000);
    write(&ws, "big.txt", &big);
    let events = run(reg().execute("read_file", json!({ "path": "big.txt" }), &ctx_for(&ws))).await;
    match error(&events) {
        ToolError::FileTooLarge { path, size, limit } => {
            assert!(path.contains("big.txt"), "{path}");
            assert_eq!(size, 2000);
            assert_eq!(limit, 1024);
        }
        other => panic!("expected FileTooLarge, got {other:?}"),
    }
}

#[tokio::test]
async fn non_utf8_content_is_read_lossily() {
    let ws = tmp_ws("read-lossy");
    write_bytes(&ws, "blob.bin", &[0xff, 0xfe, b'h', b'i']);
    let events =
        run(reg().execute("read_file", json!({ "path": "blob.bin" }), &ctx_for(&ws))).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains("hi"), "{out:?}");
    assert!(out.contains('\u{FFFD}'), "lossy marker expected: {out:?}");
}

#[tokio::test]
async fn hostile_args_are_typed_errors() {
    let ws = tmp_ws("read-args");
    for args in [
        json!({}),
        json!({ "path": 42 }),
        json!({ "path": "" }),
        json!({ "path": "a", "extra": true }),
    ] {
        let shown = args.to_string();
        let events = run(reg().execute("read_file", args, &ctx_for(&ws))).await;
        assert!(
            matches!(error(&events), ToolError::InvalidArgs { .. }),
            "{shown}"
        );
    }
    let _ = fs::read_dir(&ws).map(|_| ());
}
