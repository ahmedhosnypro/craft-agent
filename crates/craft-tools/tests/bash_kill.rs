//! The bash kill sequence: on timeout the child gets SIGTERM, then a grace
//! window, then SIGKILL — proven with short-lived `sleep` children that
//! record (or ignore) the SIGTERM.

mod common;

use common::{ctx_for, error, fast_limits, run, tmp_ws};
use craft_tools::{Registry, ToolError, ToolLimits};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn kill_limits() -> (Registry, Duration, Duration) {
    let timeout = Duration::from_millis(800);
    let grace = Duration::from_millis(400);
    (
        Registry::with_builtins(ToolLimits {
            bash_default_timeout: timeout,
            proc_grace: grace,
            ..fast_limits()
        }),
        timeout,
        grace,
    )
}

fn marker(ws: &Path) -> String {
    ws.join("term-marker.txt").display().to_string()
}

#[tokio::test]
async fn timeout_sends_sigterm_then_sigkill_when_ignored() {
    let ws = tmp_ws("kill-ignored");
    let (reg, timeout, grace) = kill_limits();
    let m = marker(&ws);
    let command = format!("trap 'echo term > \"{m}\"' TERM; while :; do :; done");
    let started = Instant::now();
    let events = run(reg.execute("bash", json!({ "command": command }), &ctx_for(&ws))).await;

    assert!(matches!(error(&events), ToolError::TimedOut { .. }));
    assert!(
        fs::read_to_string(ws.join("term-marker.txt")).is_ok_and(|c| c.contains("term")),
        "child must record the SIGTERM"
    );
    assert!(
        started.elapsed() < timeout + grace + Duration::from_secs(2),
        "kill must be prompt: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_child_hard_ignoring_sigterm_still_dies_via_sigkill() {
    let ws = tmp_ws("kill-hard-ignore");
    let (reg, timeout, grace) = kill_limits();
    let started = Instant::now();
    let events = run(reg.execute(
        "bash",
        json!({ "command": "trap '' TERM; while :; do :; done" }),
        &ctx_for(&ws),
    ))
    .await;

    assert!(matches!(error(&events), ToolError::TimedOut { .. }));
    assert!(
        started.elapsed() < timeout + grace + Duration::from_secs(2),
        "SIGKILL escalation must be prompt: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_child_exiting_on_sigterm_within_grace_reports_the_timeout() {
    let ws = tmp_ws("kill-honored");
    let (reg, _timeout, _grace) = kill_limits();
    let events = run(reg.execute(
        "bash",
        json!({ "command": "trap 'exit 0' TERM; while :; do :; done" }),
        &ctx_for(&ws),
    ))
    .await;
    assert!(
        matches!(error(&events), ToolError::TimedOut { .. }),
        "the cap was exceeded even though the child exited cleanly on SIGTERM: {events:?}"
    );
}

#[tokio::test]
async fn timeout_sends_sigterm_to_the_whole_process_group() {
    let ws = tmp_ws("kill-group");
    let (reg, _timeout, _grace) = kill_limits();
    let m = marker(&ws);
    let command =
        format!("trap 'echo termed > \"{m}\"' TERM; while :; do :; done & while :; do :; done");
    let events = run(reg.execute("bash", json!({ "command": command }), &ctx_for(&ws))).await;

    assert!(matches!(error(&events), ToolError::TimedOut { .. }));
    assert!(
        fs::read_to_string(ws.join("term-marker.txt")).is_ok_and(|c| c.contains("termed")),
        "the background job must receive the SIGTERM"
    );
}

#[tokio::test]
async fn a_fast_command_is_never_signaled() {
    let ws = tmp_ws("kill-clean");
    let (reg, _t, _g) = kill_limits();
    let events = run(reg.execute("bash", json!({ "command": "echo quick" }), &ctx_for(&ws))).await;
    common::assert_clean(&events);
    assert!(common::terminal(&events).contains("quick"));
}
