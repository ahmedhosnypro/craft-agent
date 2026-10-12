//! Symlink containment rows: dangling links, sensitive aliases,
//! and outside targets are denied; plain within-root links still work.
//! The extended secret-name bans are proven end to end here too.

mod common;

use common::{ctx_for, error, run, tmp_ws, write};
use craft_tools::{Registry, ToolError, ToolLimits};
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;

fn read(reg: &Registry, ctx: &craft_tools::ToolCtx, rel: &str) -> ToolError {
    let f = async { run(reg.execute("read_file", json!({ "path": rel }), ctx)).await };
    error(&futures::executor::block_on(f))
}

fn write_events(
    reg: &Registry,
    ctx: &craft_tools::ToolCtx,
    rel: &str,
    content: &str,
) -> Vec<Result<craft_tools::ToolEvent, ToolError>> {
    let f = async {
        run(reg.execute(
            "write_file",
            json!({ "path": rel, "content": content }),
            ctx,
        ))
        .await
    };
    futures::executor::block_on(f)
}

#[test]
fn dangling_symlink_read_and_write_are_denied() {
    let ws = tmp_ws("pathguard-dangling");
    let target_dir = ws
        .parent()
        .expect("parent")
        .join("craft-tools-dangling-targets");
    fs::create_dir_all(&target_dir).expect("target dir");
    let target = target_dir.join("payload.txt");
    symlink(&target, ws.join("link")).expect("dangling symlink");

    let reg = Registry::with_builtins(ToolLimits::default());
    let ctx = ctx_for(&ws);
    assert!(matches!(
        read(&reg, &ctx, "link"),
        ToolError::EscapesWorkspace { .. }
    ));
    let events = write_events(&reg, &ctx, "link", "escaped\n");
    assert!(matches!(error(&events), ToolError::EscapesWorkspace { .. }));
    assert!(!target.exists(), "denied write must not create the target");
    let _ = fs::remove_dir_all(&target_dir);
}

#[test]
fn midpath_dangling_symlink_is_denied() {
    let ws = tmp_ws("pathguard-dangling-mid");
    let target_dir = ws
        .parent()
        .expect("parent")
        .join("craft-tools-dangling-mid-targets");
    fs::create_dir_all(&target_dir).expect("target dir");
    let target = target_dir.join("payload.txt");
    symlink(&target, ws.join("linkdir")).expect("dangling dir symlink");

    let reg = Registry::with_builtins(ToolLimits::default());
    let ctx = ctx_for(&ws);
    assert!(matches!(
        read(&reg, &ctx, "linkdir/payload.txt"),
        ToolError::EscapesWorkspace { .. }
    ));
    let events = write_events(&reg, &ctx, "linkdir/payload.txt", "escaped\n");
    assert!(matches!(error(&events), ToolError::EscapesWorkspace { .. }));
    assert!(!target.exists(), "denied write must not create the target");
    let _ = fs::remove_dir_all(&target_dir);
}

#[test]
fn symlink_alias_to_sensitive_file_is_denied() {
    let ws = tmp_ws("pathguard-alias");
    write(&ws, ".env", "SECRET=abcdef\n");
    symlink(ws.join(".env"), ws.join("notes")).expect("alias symlink");

    let reg = Registry::with_builtins(ToolLimits::default());
    let ctx = ctx_for(&ws);
    assert!(matches!(
        read(&reg, &ctx, "notes"),
        ToolError::SensitivePath { .. }
    ));
    let events = write_events(&reg, &ctx, "notes", "overwritten\n");
    assert!(matches!(error(&events), ToolError::SensitivePath { .. }));
    assert_eq!(
        fs::read_to_string(ws.join(".env")).expect("unchanged"),
        "SECRET=abcdef\n"
    );
}

#[test]
fn plain_within_root_symlink_still_resolves() {
    let ws = tmp_ws("pathguard-plain");
    write(&ws, "readme.txt", "hello\n");
    symlink(ws.join("readme.txt"), ws.join("alias.txt")).expect("plain symlink");

    let reg = Registry::with_builtins(ToolLimits::default());
    let ctx = ctx_for(&ws);
    let f = async { run(reg.execute("read_file", json!({ "path": "alias.txt" }), &ctx)).await };
    let events = futures::executor::block_on(f);
    assert!(
        !events.iter().any(|e| e.is_err()),
        "within-root symlink read should pass: {events:?}"
    );
}

#[test]
fn extended_secret_names_are_denied_and_pub_forms_pass() {
    let ws = tmp_ws("pathguard-extended");
    for name in [
        "id_dsa",
        "server.pem",
        "site.key",
        ".netrc",
        ".npmrc",
        "id_dsa.pub",
    ] {
        write(&ws, name, "fixture\n");
    }
    let reg = Registry::with_builtins(ToolLimits::default());
    let ctx = ctx_for(&ws);
    for name in ["id_dsa", "server.pem", "site.key", ".netrc", ".npmrc"] {
        assert!(
            matches!(read(&reg, &ctx, name), ToolError::SensitivePath { .. }),
            "{name} should be denied"
        );
    }
    let f = async { run(reg.execute("read_file", json!({ "path": "id_dsa.pub" }), &ctx)).await };
    let events = futures::executor::block_on(f);
    assert!(
        !events.iter().any(|e| e.is_err()),
        "id_dsa.pub should be readable: {events:?}"
    );
}
