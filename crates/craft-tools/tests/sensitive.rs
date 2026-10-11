//! Sensitive-file hard errors and workspace-escape rejection, checked
//! per call with no caching. Exemptions: the example/sample/template env
//! forms and public `*.pub` keys.

mod common;

use common::{ctx_for, error, run, tmp_ws, write};
use craft_tools::{Registry, ToolError, ToolLimits};
use serde_json::json;
use std::fs;

fn limits() -> ToolLimits {
    ToolLimits::default()
}

fn sensitive_read(reg: &Registry, ctx: &craft_tools::ToolCtx, rel: &str) -> ToolError {
    let f = async { run(reg.execute("read_file", json!({ "path": rel }), ctx)).await };
    let events = futures::executor::block_on(f);
    error(&events)
}

#[test]
fn sensitive_paths_hard_error_on_read() {
    let ws = tmp_ws("sensitive-read");
    for name in [
        ".env",
        ".env.local",
        ".env.production",
        "id_rsa",
        "id_ed25519",
        "id_ecdsa",
        "id_rsa_key",
        "credentials.json",
        "credentials.toml",
        ".ENV",
        "sub/id_rsa",
        "sub/.env",
    ] {
        write(&ws, name, "secret-ish fixture\n");
    }
    fs::create_dir_all(ws.join(".aws")).expect("aws dir");
    write(&ws, ".aws/config", "[default]\n");
    fs::create_dir_all(ws.join(".gcp")).expect("gcp dir");
    write(&ws, ".gcp/creds.json", "{}");

    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);
    for name in [
        ".env",
        ".env.local",
        ".env.production",
        "id_rsa",
        "id_ed25519",
        "id_ecdsa",
        "id_rsa_key",
        "credentials.json",
        "credentials.toml",
        ".ENV",
        "sub/id_rsa",
        "sub/.env",
        ".aws/config",
        ".gcp/creds.json",
    ] {
        let err = sensitive_read(&reg, &ctx, name);
        assert!(
            matches!(err, ToolError::SensitivePath { .. }),
            "{name} should be denied, got {err:?}"
        );
    }
}

#[test]
fn exempted_public_forms_are_readable() {
    let ws = tmp_ws("sensitive-exempt");
    for name in [
        ".env.example",
        ".env.sample",
        ".env.template",
        "id_rsa.pub",
        "id_ed25519.pub",
        "id_ecdsa.pub",
    ] {
        write(&ws, name, "public fixture\n");
    }
    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);
    for name in [
        ".env.example",
        ".env.sample",
        ".env.template",
        "id_rsa.pub",
        "id_ed25519.pub",
        "id_ecdsa.pub",
    ] {
        let f = async { run(reg.execute("read_file", json!({ "path": name }), &ctx)).await };
        let events = futures::executor::block_on(f);
        assert!(
            !events.iter().any(|e| e.is_err()),
            "{name} should be readable, got {events:?}"
        );
    }
}

#[test]
fn non_exempt_env_spellings_stay_denied() {
    let ws = tmp_ws("sensitive-spelling");
    write(&ws, ".env.EXAMPLE", "x\n");
    write(&ws, ".envrc", "x\n");
    write(&ws, "id_rsa.PUB", "x\n");
    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);
    for name in [".env.EXAMPLE", ".envrc", "id_rsa.PUB"] {
        let err = sensitive_read(&reg, &ctx, name);
        assert!(
            matches!(err, ToolError::SensitivePath { .. }),
            "{name} should be denied, got {err:?}"
        );
    }
}

#[test]
fn write_and_edit_to_sensitive_paths_are_denied() {
    let ws = tmp_ws("sensitive-write");
    write(&ws, "id_rsa", "current key material\n");
    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);

    let events = futures::executor::block_on(run(reg.execute(
        "write_file",
        json!({ "path": ".env", "content": "leaked\n" }),
        &ctx,
    )));
    assert!(matches!(error(&events), ToolError::SensitivePath { .. }));
    assert!(
        !ws.join(".env").exists(),
        "denied write must not create the file"
    );

    let events = futures::executor::block_on(run(reg.execute(
        "edit_file",
        json!({ "path": "id_rsa", "old_string": "current", "new_string": "new" }),
        &ctx,
    )));
    assert!(matches!(error(&events), ToolError::SensitivePath { .. }));
    assert_eq!(
        fs::read_to_string(ws.join("id_rsa")).expect("unchanged"),
        "current key material\n"
    );
}

#[test]
fn guard_is_recomputed_per_call_not_cached() {
    let ws = tmp_ws("sensitive-recompute");
    write(&ws, "notes.txt", "innocent\n");
    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);

    let events = futures::executor::block_on(run(reg.execute(
        "read_file",
        json!({ "path": "notes.txt" }),
        &ctx,
    )));
    assert!(!events.iter().any(|e| e.is_err()), "first read should pass");

    fs::rename(ws.join("notes.txt"), ws.join(".env")).expect("rename to .env");
    let err = sensitive_read(&reg, &ctx, ".env");
    assert!(matches!(err, ToolError::SensitivePath { .. }));

    fs::rename(ws.join(".env"), ws.join("notes.txt")).expect("rename back");
    let events = futures::executor::block_on(run(reg.execute(
        "read_file",
        json!({ "path": "notes.txt" }),
        &ctx,
    )));
    assert!(
        !events.iter().any(|e| e.is_err()),
        "renamed-back read should pass"
    );
}

#[test]
fn relative_paths_escaping_the_workspace_are_rejected() {
    let ws = tmp_ws("sensitive-escape");
    write(&ws, "ok.txt", "inside\n");
    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);
    for path in [
        "../outside.txt",
        "sub/../../outside.txt",
        "/etc/passwd",
        "a/../../../etc",
    ] {
        let err = sensitive_read(&reg, &ctx, path);
        assert!(
            matches!(err, ToolError::EscapesWorkspace { .. }),
            "{path} should escape, got {err:?}"
        );
    }
    let events = futures::executor::block_on(run(reg.execute(
        "read_file",
        json!({ "path": "sub/../ok.txt" }),
        &ctx,
    )));
    assert!(
        !events.iter().any(|e| e.is_err()),
        "folded path should pass"
    );

    let events = futures::executor::block_on(run(reg.execute(
        "write_file",
        json!({ "path": "../out.txt", "content": "x\n" }),
        &ctx,
    )));
    assert!(matches!(error(&events), ToolError::EscapesWorkspace { .. }));
    assert!(!ws.parent().expect("parent").join("out.txt").exists());
}

#[test]
fn symlink_escape_out_of_the_workspace_is_rejected() {
    let ws = tmp_ws("sensitive-symlink");
    let outside = ws
        .parent()
        .expect("parent")
        .join("craft-tools-outside-target.txt");
    fs::write(&outside, "outside secret\n").expect("outside write");
    std::os::unix::fs::symlink(&outside, ws.join("innocent_link.txt")).expect("symlink");

    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);
    let err = sensitive_read(&reg, &ctx, "innocent_link.txt");
    assert!(
        matches!(err, ToolError::EscapesWorkspace { .. }),
        "symlink escape should be denied, got {err:?}"
    );
    let _ = fs::remove_file(&outside);
}

#[test]
fn read_through_a_workspace_symlink_stays_sensitive_aware() {
    let ws = tmp_ws("sensitive-symlink2");
    let other = tmp_ws("sensitive-symlink-target");
    write(&other, "real.txt", "content\n");
    write(&other, ".env", "SECRET=1\n");
    std::os::unix::fs::symlink(&other, ws.join("link_dir")).expect("dir symlink");

    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);
    let events = futures::executor::block_on(run(reg.execute(
        "read_file",
        json!({ "path": "link_dir/real.txt" }),
        &ctx,
    )));
    assert!(
        matches!(error(&events), ToolError::EscapesWorkspace { .. }),
        "dir-symlink read should be denied (outside root), got {events:?}"
    );
}

#[test]
fn directory_walks_skip_sensitive_files_without_failing() {
    let ws = tmp_ws("sensitive-walk");
    write(&ws, ".env", "SECRET=abcdef\n");
    write(&ws, ".env.example", "SAFE=1\n");
    write(&ws, "code.txt", "SECRET mention in code\n");
    let reg = Registry::with_builtins(limits());
    let ctx = ctx_for(&ws);

    let events = futures::executor::block_on(run(reg.execute(
        "glob",
        json!({ "pattern": ".env*" }),
        &ctx,
    )));
    let out = common::terminal(&events);
    assert!(out.contains(".env.example"), "exempt form lists: {out}");
    assert!(
        !out.contains("\n.env\n") && !out.starts_with(".env\n"),
        "sensitive hidden: {out}"
    );

    let events = futures::executor::block_on(run(reg.execute(
        "grep",
        json!({ "pattern": "SECRET", "literal": true }),
        &ctx,
    )));
    let out = common::terminal(&events);
    assert!(
        !out.contains("abcdef"),
        ".env content must never surface: {out}"
    );
    assert!(out.contains("code.txt"), "walk finds the safe file: {out}");
    assert!(out.contains("sensitive"), "skip note expected: {out}");
}
