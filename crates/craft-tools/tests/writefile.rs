//! write_file: creation, nesting, overwrite, size guard, denied paths.

mod common;

use common::{assert_clean, ctx_for, error, fast_limits, run, terminal, tmp_ws, write};
use craft_tools::{Registry, ToolError};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

#[tokio::test]
async fn writes_and_roundtrips_content() {
    let ws = tmp_ws("write-ok");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "new.txt", "content": "payload\n" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert!(
        terminal(&events).contains("new.txt"),
        "confirmation names the file"
    );
    assert_eq!(
        fs::read_to_string(ws.join("new.txt")).expect("roundtrip"),
        "payload\n"
    );
}

#[tokio::test]
async fn creates_missing_parent_directories() {
    let ws = tmp_ws("write-nested");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "a/b/c.txt", "content": "deep\n" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("a/b/c.txt")).expect("roundtrip"),
        "deep\n"
    );
}

#[tokio::test]
async fn overwrites_an_existing_file() {
    let ws = tmp_ws("write-overwrite");
    write(&ws, "old.txt", "before\n");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "old.txt", "content": "after\n" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("old.txt")).expect("roundtrip"),
        "after\n"
    );
}

#[tokio::test]
async fn oversized_content_is_rejected_untouched() {
    let ws = tmp_ws("write-large");
    let big = "y".repeat(2000);
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "big.txt", "content": big }),
        &ctx_for(&ws),
    ))
    .await;
    match error(&events) {
        ToolError::WriteTooLarge { size, limit } => {
            assert_eq!(size, 2000);
            assert_eq!(limit, 1024);
        }
        other => panic!("expected WriteTooLarge, got {other:?}"),
    }
    assert!(!ws.join("big.txt").exists(), "guard fires before any write");
}

#[tokio::test]
async fn hostile_args_are_typed_errors() {
    let ws = tmp_ws("write-args");
    for args in [
        json!({ "path": "x.txt" }),
        json!({ "content": "x" }),
        json!({ "path": "x.txt", "content": 7 }),
        json!({ "path": 1, "content": "x" }),
    ] {
        let shown = args.to_string();
        let events = run(reg().execute("write_file", args, &ctx_for(&ws))).await;
        assert!(
            matches!(error(&events), ToolError::InvalidArgs { .. }),
            "{shown}"
        );
        assert!(!ws.join("x.txt").exists(), "no file may appear for {shown}");
    }
}

#[tokio::test]
async fn append_adds_bytes_without_reading_or_a_newline() {
    let ws = tmp_ws("write-append");
    write(&ws, "f.txt", "before\n");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "f.txt", "content": "more", "mode": "append" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("appended"),
        "before\nmore"
    );
    let out = terminal(&events);
    assert!(out.contains("appended"), "{out}");
    assert!(out.contains("f.txt"), "{out}");
}

#[tokio::test]
async fn append_to_missing_file_creates_it() {
    let ws = tmp_ws("write-append-new");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "nested/new.txt", "content": "just this\n", "mode": "append" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("nested/new.txt")).expect("created"),
        "just this\n"
    );
}

#[tokio::test]
async fn append_reports_the_real_utf8_byte_count() {
    let ws = tmp_ws("write-append-count");
    write(&ws, "f.txt", "x");
    let content = "héllo→🌍";
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "f.txt", "content": content, "mode": "append" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains("appended 13 bytes"), "{out}");
}

#[tokio::test]
async fn append_respects_the_write_cap_untouched() {
    let ws = tmp_ws("write-append-cap");
    write(&ws, "f.txt", "seed\n");
    let big = "y".repeat(2000);
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "f.txt", "content": big, "mode": "append" }),
        &ctx_for(&ws),
    ))
    .await;
    match error(&events) {
        ToolError::WriteTooLarge { size, limit } => {
            assert_eq!(size, 2000);
            assert_eq!(limit, 1024);
        }
        other => panic!("expected WriteTooLarge, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("untouched"),
        "seed\n"
    );
}

#[tokio::test]
async fn append_does_not_satisfy_the_prior_read_rule() {
    let ws = tmp_ws("write-append-noread");
    write(&ws, "f.txt", "alpha\n");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "f.txt", "content": "beta", "mode": "append" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let events = run(reg().execute(
        "edit_file",
        json!({ "path": "f.txt", "old_string": "alpha", "new_string": "gamma" }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::NotReadFirst { .. }));
}

#[tokio::test]
async fn unknown_mode_is_rejected() {
    let ws = tmp_ws("write-append-mode");
    write(&ws, "f.txt", "stable\n");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "f.txt", "content": "x", "mode": "truncate" }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::InvalidArgs { .. }));
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("untouched"),
        "stable\n"
    );
}

#[tokio::test]
async fn append_still_blocks_sensitive_paths() {
    let ws = tmp_ws("write-append-sensitive");
    let events = run(reg().execute(
        "write_file",
        json!({ "path": ".env", "content": "x", "mode": "append" }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::SensitivePath { .. }));
}
