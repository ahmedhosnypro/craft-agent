//! The walker must never visit symlinks: grep and glob follow links on
//! open, so a visited symlink would read or list files outside the
//! workspace, bypassing the sensitive-path guard.

mod common;

use common::{ctx_for, run, tmp_ws, write};
use craft_tools::{Registry, ToolLimits};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(ToolLimits::default())
}

#[test]
fn walker_never_visits_symlinked_files() {
    let ws = tmp_ws("walklinks");
    let outside = ws
        .parent()
        .expect("parent")
        .join("craft-tools-walk-outside-secret.txt");
    fs::write(&outside, "outside secret\n").expect("outside write");
    std::os::unix::fs::symlink(&outside, ws.join("innocent_link.txt")).expect("symlink");
    write(&ws, "inside.txt", "inside secret\n");

    let ctx = ctx_for(&ws);
    let events = futures::executor::block_on(run(reg().execute(
        "grep",
        json!({ "pattern": "secret", "literal": true }),
        &ctx,
    )));
    let out = common::terminal(&events);
    assert!(
        !out.contains("outside secret"),
        "symlink target content must never surface: {out}"
    );
    assert!(
        out.contains("inside.txt"),
        "regular file still searched: {out}"
    );

    let events =
        futures::executor::block_on(run(reg().execute("glob", json!({ "pattern": "*" }), &ctx)));
    let out = common::terminal(&events);
    assert!(
        !out.contains("innocent_link"),
        "symlink must not be listed: {out}"
    );
    assert!(
        out.contains("inside.txt"),
        "regular file still listed: {out}"
    );
    let _ = fs::remove_file(&outside);
}
