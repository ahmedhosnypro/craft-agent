//! edit_file no-op and remedy-text rows: old==new is rejected before
//! any I/O, and the not-found / not-unique errors carry their hints.

mod common;

use common::{ctx_for, error, fast_limits, run, tmp_ws, write};
use craft_tools::{Registry, ToolCtx, ToolError, ToolEvent};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

async fn edit(ctx: &ToolCtx, args: serde_json::Value) -> Vec<Result<ToolEvent, ToolError>> {
    run(reg().execute("edit_file", args, ctx)).await
}

#[tokio::test]
async fn noop_edit_is_rejected_before_any_io() {
    let ws = tmp_ws("edithint-noop");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "alpha\n");
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "alpha", "new_string": "alpha" }),
    )
    .await;
    match error(&events) {
        ToolError::InvalidArgs { msg, .. } => {
            assert!(msg.contains("exactly the same"), "{msg}")
        }
        other => panic!("expected the no-op rejection, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(ws.join("f.txt")).unwrap_or_default(),
        "alpha\n"
    );
}

#[tokio::test]
async fn not_found_error_carries_the_reload_hint() {
    let ws = tmp_ws("edithint-nomatch");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "alpha\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "zeta", "new_string": "eta" }),
    )
    .await;
    let err = error(&events);
    assert!(matches!(err, ToolError::EditNoMatch { .. }));
    let text = err.to_string();
    assert!(text.contains("out of date"), "{text}");
    assert!(text.contains("read the file again"), "{text}");
    assert!(text.contains("reload"), "{text}");
}

#[tokio::test]
async fn ambiguous_error_names_both_remedies() {
    let ws = tmp_ws("edithint-ambiguous");
    let ctx = ctx_for(&ws);
    write(&ws, "f.txt", "x x x\n");
    let _ = run(reg().execute("read_file", json!({ "path": "f.txt" }), &ctx)).await;
    let events = edit(
        &ctx,
        json!({ "path": "f.txt", "old_string": "x", "new_string": "y" }),
    )
    .await;
    let err = error(&events);
    match &err {
        ToolError::EditAmbiguous { count, .. } => assert_eq!(*count, 3),
        other => panic!("expected EditAmbiguous, got {other:?}"),
    }
    let text = err.to_string();
    assert!(text.contains("replace_all=true"), "{text}");
    assert!(text.contains("surrounding context"), "{text}");
}
