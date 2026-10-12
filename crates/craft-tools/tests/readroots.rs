//! Read-root rows: CRAFT_AGENT_READ_ROOTS grants read_file, glob, and
//! grep a second read-only root. Writes stay workspace-confined; the
//! sensitive, escape, and per-root symlink rules hold unchanged, and a
//! degenerate knob is byte-identical to no knob.

// Corpus fixtures: expect() is the loud-test idiom here.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{assert_clean, ctx_env, ctx_for, error, fast_limits, run, terminal, tmp_ws, write};
use craft_tools::{Registry, ToolCtx, ToolError, ToolEvent};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

type Events = Vec<Result<ToolEvent, ToolError>>;

const KNOB: &str = "CRAFT_AGENT_READ_ROOTS";

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

fn lines(out: &str) -> Vec<&str> {
    out.lines().filter(|l| !l.starts_with('[')).collect()
}

fn sibling_of(ws: &Path, suffix: &str) -> PathBuf {
    let name = ws.file_name().expect("name").to_string_lossy();
    let dir = ws.parent().expect("parent").join(format!("{name}{suffix}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("sibling create");
    dir
}

fn missing_sibling(ws: &Path) -> PathBuf {
    let name = ws.file_name().expect("name").to_string_lossy();
    ws.parent().expect("parent").join(format!("{name}-missing"))
}

fn abs(dir: &Path, rel: &str) -> String {
    dir.join(rel).display().to_string()
}

fn root_str(dir: &Path) -> String {
    dir.display().to_string()
}

fn roots_ctx(ws: &Path, knob: &str) -> ToolCtx {
    ctx_env(ws, vec![(KNOB.to_string(), knob.to_string())])
}

async fn call(reg: &Registry, ctx: &ToolCtx, tool: &str, args: Value) -> Events {
    run(reg.execute(tool, args, ctx)).await
}

async fn read_out(reg: &Registry, ctx: &ToolCtx, path: &str) -> String {
    terminal(&call(reg, ctx, "read_file", json!({ "path": path })).await)
}

async fn glob_at(reg: &Registry, ctx: &ToolCtx, root: &Path) -> Events {
    let args = json!({ "pattern": "**", "path": root_str(root) });
    call(reg, ctx, "glob", args).await
}

async fn grep_at(reg: &Registry, ctx: &ToolCtx, pattern: &str, root: &Path) -> Events {
    let args = json!({ "pattern": pattern, "path": root_str(root) });
    call(reg, ctx, "grep", args).await
}
async fn deny_escape(reg: &Registry, ctx: &ToolCtx, path: &str) {
    let events = call(reg, ctx, "read_file", json!({ "path": path })).await;
    assert!(
        matches!(error(&events), ToolError::EscapesWorkspace { .. }),
        "{path}"
    );
}

async fn deny_escape_all(reg: &Registry, ctx: &ToolCtx, extra: &str) {
    for (tool, args) in [
        ("read_file", json!({ "path": format!("{extra}/x.txt") })),
        ("glob", json!({ "pattern": "*", "path": extra })),
        ("grep", json!({ "pattern": "x", "path": extra })),
    ] {
        let events = call(reg, ctx, tool, args).await;
        assert!(
            matches!(error(&events), ToolError::EscapesWorkspace { .. }),
            "{tool}"
        );
    }
}

#[tokio::test]
async fn read_file_reads_absolute_paths_in_the_extra_root() {
    let ws = tmp_ws("readroots-read-abs");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "notes.txt", "extra content\n");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let out = read_out(&reg(), &ctx, &abs(&extra, "notes.txt")).await;
    assert_eq!(out, "extra content\n");
}

#[tokio::test]
async fn glob_lists_extra_root_hits_as_absolute_paths() {
    let ws = tmp_ws("readroots-glob-abs");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "b.txt", "1");
    write(&extra, "sub/c.txt", "1");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let events = glob_at(&reg(), &ctx, &extra).await;
    assert_clean(&events);
    assert_eq!(
        lines(&terminal(&events)),
        vec![abs(&extra, "b.txt"), abs(&extra, "sub/c.txt")]
    );
}
#[tokio::test]
async fn grep_rows_over_the_extra_root_are_absolute() {
    let ws = tmp_ws("readroots-grep-abs");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "keep.txt", "needle here\nnothing\n");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let events = grep_at(&reg(), &ctx, "needle", &extra).await;
    assert_clean(&events);
    assert_eq!(
        lines(&terminal(&events)),
        vec![format!("{}:1: needle here", abs(&extra, "keep.txt"))]
    );
}

#[tokio::test]
async fn relative_input_stays_workspace_only() {
    let ws = tmp_ws("readroots-relative");
    write(&ws, "twin.txt", "workspace copy\n");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "twin.txt", "extra copy\n");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let out = read_out(&reg(), &ctx, "twin.txt").await;
    assert_eq!(out, "workspace copy\n");
    fs::remove_file(ws.join("twin.txt")).expect("workspace copy remove");
    let events = call(&reg(), &ctx, "read_file", json!({ "path": "twin.txt" })).await;
    assert!(
        matches!(error(&events), ToolError::NotFound { .. }),
        "relative read must not fall through to the extra root"
    );
}

#[tokio::test]
async fn write_file_to_the_extra_root_is_denied_and_creates_nothing() {
    let ws = tmp_ws("readroots-write-deny");
    let extra = sibling_of(&ws, "-extra");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let args = json!({ "path": abs(&extra, "out.txt"), "content": "x" });
    let events = call(&reg(), &ctx, "write_file", args).await;
    assert!(matches!(error(&events), ToolError::EscapesWorkspace { .. }));
    assert!(
        !extra.join("out.txt").exists(),
        "denied write must not create"
    );
}
#[tokio::test]
async fn edit_file_to_the_extra_root_denies_escape_not_not_read_first() {
    let ws = tmp_ws("readroots-edit-deny");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "edit.txt", "body\n");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    read_out(&reg(), &ctx, &abs(&extra, "edit.txt")).await;
    let args =
        json!({ "path": abs(&extra, "edit.txt"), "old_string": "body", "new_string": "new" });
    let events = call(&reg(), &ctx, "edit_file", args).await;
    assert!(
        matches!(error(&events), ToolError::EscapesWorkspace { .. }),
        "escape must precede the prior-read stamp check: {events:?}"
    );
}

#[tokio::test]
async fn sensitive_names_in_the_extra_root_deny_reads() {
    let ws = tmp_ws("readroots-sensitive");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, ".env", "SECRET=1\n");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let events = call(
        &reg(),
        &ctx,
        "read_file",
        json!({ "path": abs(&extra, ".env") }),
    )
    .await;
    assert!(matches!(error(&events), ToolError::SensitivePath { .. }));
}

#[tokio::test]
async fn scopes_outside_every_root_still_deny() {
    let ws = tmp_ws("readroots-scope-deny");
    let extra = sibling_of(&ws, "-extra");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    for (tool, args) in [
        ("glob", json!({ "pattern": "*", "path": "/etc" })),
        ("glob", json!({ "pattern": "*", "path": ".." })),
        ("grep", json!({ "pattern": "x", "path": "/etc" })),
        ("grep", json!({ "pattern": "x", "path": ".." })),
    ] {
        let events = call(&reg(), &ctx, tool, args).await;
        assert!(matches!(error(&events), ToolError::EscapesWorkspace { .. }));
    }
}
#[tokio::test]
async fn glob_over_the_extra_root_skips_sensitive_files() {
    let ws = tmp_ws("readroots-glob-skip");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, ".env", "SECRET=1\n");
    write(&extra, "keep.txt", "1");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let events = glob_at(&reg(), &ctx, &extra).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert_eq!(lines(&out), vec![abs(&extra, "keep.txt")]);
    assert!(out.contains("[skipped 1 sensitive files]"), "{out}");
}

#[tokio::test]
async fn extra_root_symlink_leaving_the_root_is_denied() {
    let ws = tmp_ws("readroots-link-out");
    let extra = sibling_of(&ws, "-extra");
    let outside = sibling_of(&ws, "-outside");
    write(&outside, "payload.txt", "outside\n");
    symlink(outside.join("payload.txt"), extra.join("outlink")).expect("symlink");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    deny_escape(&reg(), &ctx, &abs(&extra, "outlink")).await;
}

#[tokio::test]
async fn dangling_extra_root_symlink_is_denied() {
    let ws = tmp_ws("readroots-link-dangling");
    let extra = sibling_of(&ws, "-extra");
    symlink(extra.join("gone.txt"), extra.join("dangle")).expect("symlink");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    deny_escape(&reg(), &ctx, &abs(&extra, "dangle")).await;
}

#[tokio::test]
async fn extra_root_symlink_to_a_sensitive_target_is_denied() {
    let ws = tmp_ws("readroots-link-sensitive");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, ".env", "SECRET=1\n");
    symlink(extra.join(".env"), extra.join("alias")).expect("symlink");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let events = call(
        &reg(),
        &ctx,
        "read_file",
        json!({ "path": abs(&extra, "alias") }),
    )
    .await;
    assert!(matches!(error(&events), ToolError::SensitivePath { .. }));
}
#[tokio::test]
async fn plain_within_extra_root_symlink_still_reads() {
    let ws = tmp_ws("readroots-link-plain");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "real.txt", "through the link\n");
    symlink(extra.join("real.txt"), extra.join("link.txt")).expect("symlink");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    let out = read_out(&reg(), &ctx, &abs(&extra, "link.txt")).await;
    assert_eq!(out, "through the link\n");
}

#[tokio::test]
async fn extra_root_symlink_into_the_workspace_is_denied() {
    let ws = tmp_ws("readroots-link-cross");
    write(&ws, "ws.txt", "workspace\n");
    let extra = sibling_of(&ws, "-extra");
    symlink(ws.join("ws.txt"), extra.join("tows")).expect("symlink");
    let ctx = roots_ctx(&ws, &root_str(&extra));
    deny_escape(&reg(), &ctx, &abs(&extra, "tows")).await;
}

#[tokio::test]
async fn no_knob_keeps_today_absolute_denial() {
    let ws = tmp_ws("readroots-unset");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "x.txt", "1");
    deny_escape_all(&reg(), &ctx_for(&ws), &root_str(&extra)).await;
}

#[tokio::test]
async fn degenerate_knob_values_match_no_knob() {
    let ws = tmp_ws("readroots-degenerate");
    let extra = sibling_of(&ws, "-extra");
    write(&extra, "x.txt", "1");
    let oversized = format!("{},{}", root_str(&extra), "x".repeat(4200));
    for ctx in [
        roots_ctx(&ws, ""),
        roots_ctx(&ws, "sub"),
        roots_ctx(&ws, &oversized),
    ] {
        deny_escape_all(&reg(), &ctx, &root_str(&extra)).await;
    }
}

#[tokio::test]
async fn at_most_four_roots_are_honored() {
    let ws = tmp_ws("readroots-cap");
    let mut roots = Vec::new();
    for idx in 1..=5 {
        let root = sibling_of(&ws, &format!("-r{idx}"));
        write(&root, "file.txt", &format!("r{idx}\n"));
        roots.push(root);
    }
    let knob = roots
        .iter()
        .map(|r| root_str(r))
        .collect::<Vec<_>>()
        .join(",");
    let ctx = roots_ctx(&ws, &knob);
    let out = read_out(&reg(), &ctx, &abs(&roots[3], "file.txt")).await;
    assert_eq!(out, "r4\n");
    deny_escape(&reg(), &ctx, &abs(&roots[4], "file.txt")).await;
}

#[tokio::test]
async fn nonexistent_root_is_tolerated_as_not_found() {
    let ws = tmp_ws("readroots-missing-root");
    let missing = missing_sibling(&ws);
    let ctx = roots_ctx(&ws, &root_str(&missing));
    let path = format!("{}/file.txt", root_str(&missing));
    let events = call(&reg(), &ctx, "read_file", json!({ "path": path })).await;
    assert!(matches!(error(&events), ToolError::NotFound { .. }));
}
