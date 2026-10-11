//! bash tool: timeout caps, closed stdin, bounded output with a truncation
//! marker, typed nonzero-exit errors. The kill-sequence corpus lives in
//! bash_kill.rs.

mod common;

use common::{assert_clean, ctx_for, error, fast_limits, progress, run, terminal, tmp_ws};
use craft_tools::{Registry, TRUNCATION_MARKER, ToolError, ToolLimits};
use serde_json::json;
use std::time::{Duration, Instant};

fn reg(limits: ToolLimits) -> Registry {
    Registry::with_builtins(limits)
}

#[tokio::test]
async fn simple_success_streams_progress_before_terminal() {
    let ws = tmp_ws("bash-ok");
    let events = run(reg(fast_limits()).execute(
        "bash",
        json!({ "command": "echo hello; echo world" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains("hello") && out.contains("world"), "{out}");
    let chunks = progress(&events);
    assert!(
        !chunks.is_empty(),
        "stdout must arrive as progress: {events:?}"
    );
    assert!(
        chunks.iter().any(|c| c.contains("hello")),
        "progress carries stdout: {chunks:?}"
    );
    assert!(matches!(
        events.last(),
        Some(Ok(craft_tools::ToolEvent::Terminal(_)))
    ));
}

#[tokio::test]
async fn stdin_is_closed_so_reads_see_eof() {
    let ws = tmp_ws("bash-stdin");
    let events = run(reg(fast_limits()).execute(
        "bash",
        json!({ "command": "read x; echo rc=$?" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert!(
        terminal(&events).contains("rc=1"),
        "read must hit EOF immediately"
    );
}

#[tokio::test]
async fn nonzero_exit_is_a_typed_error_carrying_the_code() {
    let ws = tmp_ws("bash-exit");
    let events = run(reg(fast_limits()).execute(
        "bash",
        json!({ "command": "echo boom 1>&2; exit 7" }),
        &ctx_for(&ws),
    ))
    .await;
    match error(&events) {
        ToolError::NonZeroExit { code, stderr } => {
            assert_eq!(code, Some(7));
            assert!(stderr.contains("boom"), "{stderr}");
        }
        other => panic!("expected NonZeroExit, got {other:?}"),
    }
}

#[tokio::test]
async fn default_timeout_caps_a_hanging_command() {
    let ws = tmp_ws("bash-default-timeout");
    let started = Instant::now();
    let events =
        run(reg(fast_limits()).execute("bash", json!({ "command": "sleep 30" }), &ctx_for(&ws)))
            .await;
    assert!(matches!(error(&events), ToolError::TimedOut { .. }));
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "default timeout must be fast: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn requested_timeout_over_the_cap_is_rejected_before_spawn() {
    let ws = tmp_ws("bash-over-max");
    let limits = fast_limits();
    let events = run(reg(limits).execute(
        "bash",
        json!({ "command": "echo x", "timeout_secs": 5000 }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::InvalidArgs { .. }));
}

#[tokio::test]
async fn requested_timeout_within_cap_is_enforced() {
    let ws = tmp_ws("bash-custom-timeout");
    let events = run(reg(fast_limits()).execute(
        "bash",
        json!({ "command": "sleep 30", "timeout_secs": 1 }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::TimedOut { .. }));
}

#[tokio::test]
async fn oversized_output_is_truncated_with_a_marker_never_oom() {
    let ws = tmp_ws("bash-truncate");
    let limits = fast_limits();
    let cap = limits.proc_out_cap;
    let events = run(reg(limits).execute(
        "bash",
        json!({ "command": "for i in $(seq 1 500); do echo \"0123456789_$i\"; done" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains(TRUNCATION_MARKER), "{out}");
    assert!(
        out.len() <= cap + TRUNCATION_MARKER.len() + 64,
        "output {} exceeds cap {cap}",
        out.len()
    );
}

#[tokio::test]
async fn a_single_huge_line_is_bounded() {
    let ws = tmp_ws("bash-huge-line");
    let limits = fast_limits();
    let cap = limits.proc_out_cap;
    let events = run(reg(limits).execute(
        "bash",
        json!({ "command": "head -c 200000 /dev/zero | tr '\\0' 'x'" }),
        &ctx_for(&ws),
    ))
    .await;
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
async fn non_utf8_output_is_lossy_never_an_error() {
    let ws = tmp_ws("bash-nonutf8");
    let events = run(reg(fast_limits()).execute(
        "bash",
        json!({ "command": "printf '\\xff\\xfe plain tail'" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    assert!(terminal(&events).contains("plain tail"));
}

#[tokio::test]
async fn oversize_command_string_is_rejected() {
    let ws = tmp_ws("bash-long-command");
    let events = run(reg(fast_limits()).execute(
        "bash",
        json!({ "command": "echo 0123456789".repeat(1000) }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(matches!(error(&events), ToolError::InvalidArgs { .. }));
}

#[tokio::test]
async fn missing_command_is_invalid_args() {
    let ws = tmp_ws("bash-no-command");
    let events =
        run(reg(fast_limits()).execute("bash", json!({ "timeout_secs": 1 }), &ctx_for(&ws))).await;
    assert!(matches!(error(&events), ToolError::InvalidArgs { .. }));

    let events =
        run(reg(fast_limits()).execute("bash", json!({ "command": 42 }), &ctx_for(&ws))).await;
    assert!(matches!(error(&events), ToolError::InvalidArgs { .. }));
}

#[tokio::test]
async fn stderr_surfaces_in_the_result_and_progress() {
    let ws = tmp_ws("bash-stderr");
    let events = run(reg(fast_limits()).execute(
        "bash",
        json!({ "command": "echo out; echo errline 1>&2" }),
        &ctx_for(&ws),
    ))
    .await;
    assert_clean(&events);
    let out = terminal(&events);
    assert!(out.contains("out") && out.contains("errline"), "{out}");
    assert!(
        progress(&events).iter().any(|c| c.contains("errline")),
        "stderr must stream as progress: {:?}",
        progress(&events)
    );
}

#[tokio::test]
async fn missing_bash_binary_is_a_typed_spawn_error() {
    let ws = tmp_ws("bash-missing");
    let ctx = common::ctx_env(
        &ws,
        vec![("PATH".to_string(), "/nonexistent-dir".to_string())],
    );
    let events =
        run(reg(fast_limits()).execute("bash", json!({ "command": "echo x" }), &ctx)).await;
    assert!(matches!(error(&events), ToolError::Spawn { .. }));
}
