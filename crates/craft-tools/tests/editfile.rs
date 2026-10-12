//! edit_file: prior-read requirement, exact-match replacement, ambiguity,
//! and changed-on-disk detection. One ToolCtx per test — the read stamp
//! lives in the ctx, so a fresh ctx per call would lose it (that
//! isolation is the runtime's guarantee, so the corpus pins it).

mod common;

use common::{assert_clean, ctx_for, error, fast_limits, run, terminal, tmp_ws, write};
use craft_tools::{Registry, ToolCtx, ToolError};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

async fn edit(
    ctx: &ToolCtx,
    args: serde_json::Value,
) -> Vec<Result<craft_tools::ToolEvent, ToolError>> {
    run(reg().execute("edit_file", args, ctx)).await
}

#[tokio::test]
async fn edit_requires_a_prior_read() {
    let ws = tmp_ws("edit-noread");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "alpha beta\n");
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "alpha", "new_string": "gamma" }),
    )
    .await;
    assert!(matches!(error(&events), ToolError::NotReadFirst { .. }));
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("unchanged"),
        "alpha beta\n"
    );
}

#[tokio::test]
async fn writing_the_file_does_not_satisfy_the_prior_read_rule() {
    let ws = tmp_ws("edit-afterwrite");
    let ctx = ctx_for(&ws);
    let events = run(reg().execute(
        "write_file",
        json!({ "path": "f.txt", "content": "alpha\n" }),
        &ctx,
    ))
    .await;
    assert_clean(&events);
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "alpha", "new_string": "gamma" }),
    )
    .await;
    assert!(matches!(error(&events), ToolError::NotReadFirst { .. }));
}

#[tokio::test]
async fn exact_match_edit_replaces_once() {
    let ws = tmp_ws("edit-ok");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "alpha beta gamma\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "beta", "new_string": "BETA" }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("edited"),
        "alpha BETA gamma\n"
    );
}

#[tokio::test]
async fn consecutive_edits_after_one_read_are_allowed() {
    let ws = tmp_ws("edit-twice");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "one two\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "one", "new_string": "ONE" }),
    )
    .await;
    assert_clean(&events);
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "two", "new_string": "TWO" }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("edited twice"),
        "ONE TWO\n"
    );
}

#[tokio::test]
async fn missing_match_is_a_typed_error() {
    let ws = tmp_ws("edit-nomatch");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "alpha\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "zeta", "new_string": "eta" }),
    )
    .await;
    assert!(matches!(error(&events), ToolError::EditNoMatch { .. }));
}

#[tokio::test]
async fn ambiguous_match_requires_uniqueness_or_replace_all() {
    let ws = tmp_ws("edit-ambiguous");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "x x x\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;

    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "x", "new_string": "y" }),
    )
    .await;
    match error(&events) {
        ToolError::EditAmbiguous { count, .. } => assert_eq!(count, 3),
        other => panic!("expected EditAmbiguous, got {other:?}"),
    }

    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "x", "new_string": "y", "replace_all": true }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("replaced all"),
        "y y y\n"
    );
    assert!(
        terminal(&events).contains("3"),
        "confirmation reports the count"
    );
}

#[tokio::test]
async fn empty_old_string_is_rejected() {
    let ws = tmp_ws("edit-empty");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "data\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "", "new_string": "z" }),
    )
    .await;
    assert!(matches!(error(&events), ToolError::InvalidArgs { .. }));
}

#[tokio::test]
async fn external_modification_after_read_is_detected() {
    let ws = tmp_ws("edit-changed");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "original\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;

    fs::write(ws.join("f.txt"), "replaced externally\n").expect("outside write");
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "replaced externally", "new_string": "edited" }),
    )
    .await;
    assert!(
        matches!(error(&events), ToolError::ChangedOnDisk { .. }),
        "a matching old_string must not rescue a changed file: {events:?}"
    );
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("unchanged"),
        "replaced externally\n"
    );
}

#[tokio::test]
async fn rereading_after_external_change_reenables_edit() {
    let ws = tmp_ws("edit-reread");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "original\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    fs::write(ws.join("f.txt"), "fresh content\n").expect("outside write");

    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "fresh", "new_string": "crisp" }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("edited"),
        "crisp content\n"
    );
}

#[tokio::test]
async fn edit_result_over_the_write_cap_is_rejected_untouched() {
    let ws = tmp_ws("edit-blowup");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "aaa\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({
            "path": "f.txt",
            "old_string": "a",
            "new_string": "z".repeat(2000),
            "replace_all": true
        }),
    )
    .await;
    match error(&events) {
        ToolError::FileTooLarge { size, limit, .. } => {
            assert_eq!(size, 4 - 3 + 3 * 2000);
            assert_eq!(limit, 1024);
        }
        other => panic!("expected FileTooLarge, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("unchanged"),
        "aaa\n"
    );
}

#[tokio::test]
async fn replace_all_skips_overlapping_occurrences() {
    let ws = tmp_ws("edit-overlap");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "aaaa\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "aa", "new_string": "y", "replace_all": true }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("edited"),
        "yy\n"
    );
}

#[tokio::test]
async fn replace_all_finds_matches_after_a_partial_prefix() {
    let ws = tmp_ws("edit-prefix");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "aabaaab\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "aab", "new_string": "X", "replace_all": true }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).expect("edited"),
        "XaX\n"
    );
}
