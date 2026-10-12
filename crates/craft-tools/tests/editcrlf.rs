//! edit_file's line-ending model: pure-CRLF files match on an LF view
//! and write back CRLF without doubling CRs; mixed or lone-CR files are
//! refused with a remedy hint and stay byte-exact.

mod common;

use common::{assert_clean, ctx_for, error, fast_limits, run, tmp_ws, write_bytes};
use craft_tools::{Registry, ToolCtx, ToolError, ToolEvent};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

async fn edit(ctx: &ToolCtx, args: serde_json::Value) -> Vec<Result<ToolEvent, ToolError>> {
    run(reg().execute("edit_file", args, ctx)).await
}

async fn read_first(ctx: &ToolCtx, path: &str) {
    let events = run(reg().execute("read_file", json!({ "path": path }), ctx)).await;
    assert_clean(&events);
}

fn file_bytes(ws: &std::path::Path, rel: &str) -> Vec<u8> {
    fs::read(ws.join(rel)).unwrap_or_default()
}

#[tokio::test]
async fn crlf_file_matches_lf_view_and_writes_crlf() {
    let ws = tmp_ws("crlf-lfview");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"alpha\r\nbeta\r\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "beta", "new_string": "BETA" }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(file_bytes(&ws, "f.txt"), b"alpha\r\nBETA\r\n");
}

#[tokio::test]
async fn crlf_old_string_with_pairs_also_matches() {
    let ws = tmp_ws("crlf-crlfold");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"alpha\r\nbeta\r\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "alpha\r\nbeta", "new_string": "alpha\r\ngamma" }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(file_bytes(&ws, "f.txt"), b"alpha\r\ngamma\r\n");
}

#[tokio::test]
async fn crlf_new_string_never_doubles_carriage_returns() {
    let ws = tmp_ws("crlf-nodouble");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\r\nb\r\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "b", "new_string": "x\r\ny" }),
    )
    .await;
    assert_clean(&events);
    let bytes = file_bytes(&ws, "f.txt");
    assert_eq!(bytes, b"a\r\nx\r\ny\r\n");
    assert!(
        !bytes.windows(2).any(|w| w == b"\r\r"),
        "a CR must never double: {bytes:?}"
    );
}

#[tokio::test]
async fn crlf_replace_all_stays_pure_crlf() {
    let ws = tmp_ws("crlf-replaceall");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\r\na\r\na\r\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "a", "new_string": "b", "replace_all": true }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(file_bytes(&ws, "f.txt"), b"b\r\nb\r\nb\r\n");
}

#[tokio::test]
async fn consecutive_crlf_edits_hold_the_stamp() {
    let ws = tmp_ws("crlf-twice");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"one\r\ntwo\r\n");
    read_first(&ctx, "f.txt").await;
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
    assert_eq!(file_bytes(&ws, "f.txt"), b"ONE\r\nTWO\r\n");
}

#[tokio::test]
async fn lf_files_stay_byte_exact() {
    let ws = tmp_ws("crlf-lfonly");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\nb\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "b", "new_string": "B" }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(file_bytes(&ws, "f.txt"), b"a\nB\n");
}

#[tokio::test]
async fn crlf_file_with_unterminated_last_line_is_still_crlf() {
    let ws = tmp_ws("crlf-notrail");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\r\nb");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "b", "new_string": "B" }),
    )
    .await;
    assert_clean(&events);
    assert_eq!(file_bytes(&ws, "f.txt"), b"a\r\nB");
}

#[tokio::test]
async fn mixed_endings_file_is_refused_with_a_hint() {
    let ws = tmp_ws("crlf-mixed");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\r\nb\nc\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "b", "new_string": "B" }),
    )
    .await;
    let err = error(&events);
    assert!(matches!(err, ToolError::EditMixedEndings { .. }), "{err:?}");
    let text = err.to_string();
    assert!(text.contains("mixed"), "{text}");
    assert!(text.contains("write_file"), "{text}");
    assert_eq!(file_bytes(&ws, "f.txt"), b"a\r\nb\nc\n");
}

#[tokio::test]
async fn lone_cr_file_is_refused_with_a_hint() {
    let ws = tmp_ws("crlf-lonecr");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\rb\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "a", "new_string": "A" }),
    )
    .await;
    let err = error(&events);
    assert!(matches!(err, ToolError::EditMixedEndings { .. }), "{err:?}");
    let text = err.to_string();
    assert!(text.contains("mixed"), "{text}");
    assert!(text.contains("write_file"), "{text}");
    assert_eq!(file_bytes(&ws, "f.txt"), b"a\rb\n");
}

#[tokio::test]
async fn mixed_refusal_loses_to_the_prior_read_rule() {
    let ws = tmp_ws("crlf-mixed-noread");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\r\nb\nc\n");
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "b", "new_string": "B" }),
    )
    .await;
    assert!(matches!(error(&events), ToolError::NotReadFirst { .. }));
    assert_eq!(file_bytes(&ws, "f.txt"), b"a\r\nb\nc\n");
}

#[tokio::test]
async fn lf_only_strings_do_not_match_a_mixed_file() {
    let ws = tmp_ws("crlf-mixed-raw");
    let ctx = ctx_for(&ws);
    write_bytes(&ws, "f.txt", b"a\r\nb\nc\n");
    read_first(&ctx, "f.txt").await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "a\nb", "new_string": "z" }),
    )
    .await;
    let err = error(&events);
    assert!(
        matches!(err, ToolError::EditMixedEndings { .. }),
        "mixed files are refused before matching: {err:?}"
    );
    assert_eq!(file_bytes(&ws, "f.txt"), b"a\r\nb\nc\n");
}
