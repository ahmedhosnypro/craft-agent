//! gh tool: shells out to the gh CLI with a scrubbed env and GH_TOKEN.
//! The corpus drives a FAKE gh binary on PATH — never the network.

mod common;

use common::{
    assert_clean, ctx_env, error, fake_child_env, fake_gh, fast_limits, run, terminal, tmp_ws,
};
use craft_tools::{Registry, TRUNCATION_MARKER, ToolError};
use serde_json::json;
use std::fs;

const GH_SCRIPT: &str = r#"case "$FAKE_GH_MODE" in
  ok) echo "hello from gh" ;;
  json) echo '{"items":[{"n":1}]}' ;;
  jsonbad) echo '{"broken' ;;
  fail) echo "gh: no such repo (HTTP 404)" >&2; exit 2 ;;
  huge) head -c 100000 /dev/zero | tr '\0' 'g' ;;
  hugeline) head -c 50000 /dev/zero | tr '\0' 'L' ;;
  nonutf8) printf '\xff\xfe gh-tail' ;;
esac"#;

#[allow(clippy::expect_used)] // fixture builder outside a test fn
fn ws_with_gh(name: &str, mode: &str) -> (std::path::PathBuf, Registry, craft_tools::ToolCtx) {
    let ws = tmp_ws(name);
    let bin = ws.join("bin");
    fs::create_dir_all(&bin).expect("bin dir");
    fake_gh(&bin, GH_SCRIPT);
    let mut env = fake_child_env(Some(bin));
    if let Some(slot) = env.iter_mut().find(|(k, _)| k == "FAKE_GH_MODE") {
        slot.1 = mode.to_string();
    }
    let ctx = ctx_env(&ws, env);
    (ws, Registry::with_builtins(fast_limits()), ctx)
}

#[tokio::test]
async fn success_output_is_returned() {
    let (_ws, reg, ctx) = ws_with_gh("gh-ok", "ok");
    let events = run(reg.execute("gh", json!({ "args": ["pr", "list"] }), &ctx)).await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "hello from gh\n");
}

#[tokio::test]
async fn json_flag_output_must_parse() {
    let (_ws, reg, ctx) = ws_with_gh("gh-json", "json");
    let events = run(reg.execute(
        "gh",
        json!({ "args": ["pr", "list", "--json", "number"] }),
        &ctx,
    ))
    .await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "{\"items\":[{\"n\":1}]}\n");

    let (_ws, reg, ctx) = ws_with_gh("gh-jsonbad", "jsonbad");
    let events = run(reg.execute(
        "gh",
        json!({ "args": ["pr", "list", "--json", "number"] }),
        &ctx,
    ))
    .await;
    match error(&events) {
        ToolError::GhBadJson { snippet } => assert!(snippet.contains("broken"), "{snippet}"),
        other => panic!("expected GhBadJson, got {other:?}"),
    }
}

#[tokio::test]
async fn failure_output_maps_to_a_typed_error_with_code_and_stderr() {
    let (_ws, reg, ctx) = ws_with_gh("gh-fail", "fail");
    let events = run(reg.execute("gh", json!({ "args": ["pr", "view", "999"] }), &ctx)).await;
    match error(&events) {
        ToolError::GhFailed { code, stderr } => {
            assert_eq!(code, Some(2));
            assert!(stderr.contains("no such repo"), "{stderr}");
        }
        other => panic!("expected GhFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn huge_output_is_truncated_with_a_marker_never_oom() {
    let (_ws, reg, ctx) = ws_with_gh("gh-huge", "huge");
    let cap = fast_limits().proc_out_cap;
    let events = run(reg.execute("gh", json!({ "args": ["pr", "list"] }), &ctx)).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains(TRUNCATION_MARKER), "{out}");
    assert!(
        out.len() <= cap + TRUNCATION_MARKER.len() + 64,
        "len {}",
        out.len()
    );
}

#[tokio::test]
async fn a_single_huge_line_is_bounded() {
    let (_ws, reg, ctx) = ws_with_gh("gh-hugeline", "hugeline");
    let cap = fast_limits().proc_out_cap;
    let events = run(reg.execute("gh", json!({ "args": ["pr", "list"] }), &ctx)).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains(TRUNCATION_MARKER), "{out}");
    assert!(
        out.len() <= cap + TRUNCATION_MARKER.len() + 64,
        "len {}",
        out.len()
    );
}

#[tokio::test]
async fn malformed_json_snippets_stay_bounded() {
    let (_ws, reg, ctx) = ws_with_gh("gh-huge-json", "huge");
    let events = run(reg.execute(
        "gh",
        json!({ "args": ["pr", "list", "--json", "number"] }),
        &ctx,
    ))
    .await;
    match error(&events) {
        ToolError::GhBadJson { snippet } => assert!(snippet.len() <= 512, "len {}", snippet.len()),
        other => panic!("expected GhBadJson for non-JSON output, got {other:?}"),
    }
}

#[tokio::test]
async fn non_utf8_output_is_lossy_never_an_error() {
    let (_ws, reg, ctx) = ws_with_gh("gh-nonutf8", "nonutf8");
    let events = run(reg.execute("gh", json!({ "args": ["pr", "list"] }), &ctx)).await;
    assert_clean(&events);
    assert!(
        terminal(&events).contains("gh-tail"),
        "lossy output keeps the readable tail"
    );
}

#[tokio::test]
async fn missing_gh_binary_is_a_typed_spawn_error() {
    let ws = tmp_ws("gh-missing");
    let ctx = ctx_env(
        &ws,
        vec![("PATH".to_string(), "/nonexistent-dir".to_string())],
    );
    let reg = Registry::with_builtins(fast_limits());
    let events = run(reg.execute("gh", json!({ "args": ["auth", "status"] }), &ctx)).await;
    assert!(matches!(error(&events), ToolError::Spawn { .. }));
}

#[tokio::test]
async fn argv_is_capped_and_args_validated() {
    let (_ws, reg, ctx) = ws_with_gh("gh-args", "ok");
    let events = run(reg.execute(
        "gh",
        json!({ "args": (0..10).map(|i| format!("a{i}")).collect::<Vec<_>>() }),
        &ctx,
    ))
    .await;
    assert!(matches!(error(&events), ToolError::InvalidArgs { .. }));

    for args in [
        json!({}),
        json!({ "args": [] }),
        json!({ "args": "pr list" }),
        json!({ "args": [1, 2] }),
    ] {
        let shown = args.to_string();
        let events = run(reg.execute("gh", args, &ctx)).await;
        assert!(
            matches!(error(&events), ToolError::InvalidArgs { .. }),
            "{shown}"
        );
    }
}

#[tokio::test]
async fn gh_token_reaches_the_child() {
    let (_ws, reg, ctx) = ws_with_gh("gh-token", "env");
    let script = "echo \"GH_TOKEN=$GH_TOKEN\"";
    let bin = _ws.join("bin");
    fake_gh(&bin, script);
    let events = run(reg.execute("gh", json!({ "args": ["auth", "status"] }), &ctx)).await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "GH_TOKEN=ghp-fake-token\n");
}

#[tokio::test]
async fn output_truncation_marks_the_marker() {
    let (_ws, reg, ctx) = ws_with_gh("gh-truncmark", "huge");
    let events = run(reg.execute("gh", json!({ "args": ["pr", "list"] }), &ctx)).await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains(TRUNCATION_MARKER), "{out}");
}
