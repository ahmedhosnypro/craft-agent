//! Child-env scrubbing: provider-key variables never reach tool children
//! while GH_TOKEN stays. The pure filter is driven on synthetic maps; the
//! end-to-end proof runs `env` through bash and a token-printing fake gh.

mod common;

use common::{assert_clean, ctx_env, fake_child_env, run, terminal, tmp_ws};
use craft_tools::envscrub::scrub;
use craft_tools::{Registry, ToolLimits};
use serde_json::json;

fn pairs(map: &[(&str, &str)]) -> Vec<(String, String)> {
    map.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn names(env: Vec<(String, String)>) -> Vec<String> {
    let mut kept: Vec<String> = scrub(env).into_iter().map(|(k, _)| k).collect();
    kept.sort_unstable();
    kept
}

#[test]
fn provider_key_families_are_removed() {
    let kept = names(pairs(&[
        ("OPENAI_API_KEY", "a"),
        ("ANTHROPIC_API_KEY", "b"),
        ("GOOGLE_API_KEY", "c"),
        ("STRIPE_API_KEY", "d"),
        ("MY_SERVICE_API_KEY", "e"),
        ("API_KEY", "f"),
        ("TEST_BYOK_API_KEY", "g"),
        ("TEST_BYOK_BASE_URL", "h"),
        ("TEST_BYOK_VISION_MODEL", "i"),
        ("GH_TOKEN", "kept"),
        ("PATH", "kept"),
        ("HOME", "kept"),
        ("OPENAI_BASE_URL", "kept"),
        ("SOME_SECRET", "kept"),
    ]));
    assert_eq!(
        kept,
        vec!["GH_TOKEN", "HOME", "OPENAI_BASE_URL", "PATH", "SOME_SECRET"]
    );
}

#[test]
fn scrub_preserves_order_and_values() {
    let env = pairs(&[("ZED", "1"), ("OPENAI_API_KEY", "x"), ("GH_TOKEN", "tok")]);
    let scrubbed = scrub(env);
    assert_eq!(
        scrubbed,
        vec![
            ("ZED".to_string(), "1".to_string()),
            ("GH_TOKEN".to_string(), "tok".to_string())
        ]
    );
}

#[tokio::test]
async fn bash_child_env_has_no_provider_keys_but_keeps_gh_token() {
    let ws = tmp_ws("envscrub-bash");
    let reg = Registry::with_builtins(ToolLimits::default());
    let ctx = ctx_env(&ws, fake_child_env(None));
    let events = run(reg.execute("bash", json!({ "command": "env" }), &ctx)).await;
    assert_clean(&events);
    let out = terminal(&events);

    assert!(out.contains("GH_TOKEN=ghp-fake-token"), "{out}");
    assert!(
        out.contains("FAKE_GH_MODE=ok"),
        "unrelated vars pass through: {out}"
    );
    for banned in [
        "OPENAI_API_KEY=",
        "ANTHROPIC_API_KEY=",
        "GOOGLE_API_KEY=",
        "STRIPE_API_KEY=",
        "API_KEY=",
        "TEST_BYOK_API_KEY=",
        "TEST_BYOK_BASE_URL=",
        "sk-fake",
        "fake-byok",
    ] {
        assert!(!out.contains(banned), "child env leaked {banned}: {out}");
    }
}

#[tokio::test]
async fn gh_child_env_scrubbed_and_gh_token_passed_through() {
    let ws = tmp_ws("envscrub-gh");
    let bin = ws.join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    common::fake_gh(
        &bin,
        "case \"$FAKE_GH_MODE\" in env) echo \"GH_TOKEN=$GH_TOKEN\" ;; esac",
    );

    let reg = Registry::with_builtins(ToolLimits::default());
    let mut env = fake_child_env(Some(bin.clone()));
    env.retain(|(k, _)| k != "FAKE_GH_MODE");
    env.push(("FAKE_GH_MODE".to_string(), "env".to_string()));
    let ctx = ctx_env(&ws, env);
    let events = run(reg.execute("gh", json!({ "args": ["pr", "list"] }), &ctx)).await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "GH_TOKEN=ghp-fake-token\n");
}

#[tokio::test]
async fn scrubbing_happens_per_call_not_at_construction() {
    let ws = tmp_ws("envscrub-percall");
    let reg = Registry::with_builtins(ToolLimits::default());

    let with_key = ctx_env(&ws, fake_child_env(None));
    let events = run(reg.execute("bash", json!({ "command": "env" }), &with_key)).await;
    assert!(!terminal(&events).contains("OPENAI_API_KEY="));

    let no_key = ctx_env(
        &ws,
        vec![(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )],
    );
    let events = run(reg.execute("bash", json!({ "command": "env" }), &no_key)).await;
    assert!(!terminal(&events).contains("OPENAI_API_KEY="));
    assert!(!terminal(&events).contains("GH_TOKEN="));
}
