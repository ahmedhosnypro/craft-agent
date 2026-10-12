//! gh write-restriction rows: CRAFT_AGENT_WRITE_REPO declares the one
//! repository gh may write to. Read verbs always pass; write verbs must
//! name that repo via -R/--repo (or a repos/<owner>/<repo> API path);
//! `gh auth setup-git` is denied while the knob is set. Pass rows run a
//! fake gh binary — never the network.

mod common;

use common::{
    assert_clean, ctx_env, error, fake_child_env, fake_gh, fast_limits, run, terminal, tmp_ws,
};
use craft_tools::{Registry, ToolError};
use serde_json::json;
use std::fs;

fn reg() -> Registry {
    Registry::with_builtins(fast_limits())
}

async fn gh(
    name: &str,
    knob: Option<&str>,
    args: &[&str],
) -> Vec<Result<craft_tools::ToolEvent, ToolError>> {
    let ws = tmp_ws(name);
    let bin = ws.join("bin");
    let _ = fs::create_dir_all(&bin);
    fake_gh(&bin, "echo fake-gh-ran");
    let mut pairs = fake_child_env(Some(bin));
    if let Some(value) = knob {
        pairs.push(("CRAFT_AGENT_WRITE_REPO".to_string(), value.to_string()));
    }
    let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run(reg().execute("gh", json!({ "args": owned }), &ctx_env(&ws, pairs))).await
}

async fn gh_ok(name: &str, knob: Option<&str>, args: &[&str]) {
    let events = gh(name, knob, args).await;
    assert_clean(&events);
    assert_eq!(terminal(&events), "fake-gh-ran\n");
}

async fn gh_err(name: &str, knob: Option<&str>, args: &[&str]) -> ToolError {
    let events = gh(name, knob, args).await;
    error(&events)
}

const WRITE_REPO: &str = "acme/widgets";

#[tokio::test]
async fn read_verbs_pass_for_any_repo() {
    gh_ok(
        "ghg-read-pr",
        Some(WRITE_REPO),
        &["pr", "view", "-R", "acme/other"],
    )
    .await;
    gh_ok(
        "ghg-read-issue",
        Some(WRITE_REPO),
        &["issue", "list", "-R", "zeta/analyzed"],
    )
    .await;
    gh_ok(
        "ghg-read-api",
        Some(WRITE_REPO),
        &["api", "repos/zeta/analyzed/pulls"],
    )
    .await;
}

#[tokio::test]
async fn write_to_the_write_repo_passes() {
    gh_ok(
        "ghg-write-ok",
        Some(WRITE_REPO),
        &["pr", "create", "-R", "acme/widgets"],
    )
    .await;
    gh_ok(
        "ghg-write-long",
        Some(WRITE_REPO),
        &["issue", "comment", "7", "-R", "acme/widgets", "-b", "x"],
    )
    .await;
}

#[tokio::test]
async fn write_to_another_repo_denies_naming_knob_and_target() {
    let err = gh_err(
        "ghg-write-other",
        Some(WRITE_REPO),
        &["pr", "create", "-R", "acme/other"],
    )
    .await;
    match &err {
        ToolError::GhWriteDenied { target } => assert_eq!(target, "acme/other"),
        other => panic!("expected GhWriteDenied, got {other:?}"),
    }
    let text = err.to_string();
    assert!(text.contains("CRAFT_AGENT_WRITE_REPO"), "{text}");
    assert!(text.contains("acme/other"), "{text}");
}

#[tokio::test]
async fn write_without_an_explicit_target_denies() {
    let err = gh_err("ghg-write-notarget", Some(WRITE_REPO), &["pr", "create"]).await;
    assert!(matches!(err, ToolError::GhWriteUntargeted), "{err:?}");
}

#[tokio::test]
async fn push_literal_denies_as_an_unrecognized_write() {
    let err = gh_err("ghg-push", Some(WRITE_REPO), &["push"]).await;
    assert!(matches!(err, ToolError::GhWriteUntargeted), "{err:?}");
}

#[tokio::test]
async fn write_verb_families_are_gated() {
    for (name, args) in [
        ("close", vec!["pr", "close", "1", "-R", "acme/other"]),
        ("edit", vec!["pr", "edit", "1", "-R", "acme/other"]),
        ("merge", vec!["pr", "merge", "1", "-R", "acme/other"]),
        ("comment", vec!["pr", "comment", "1", "-R", "acme/other"]),
        (
            "issue",
            vec!["issue", "create", "-R", "acme/other", "-t", "x"],
        ),
        ("repo", vec!["repo", "edit", "acme/other", "--x"]),
        (
            "release",
            vec!["release", "create", "v1", "-R", "acme/other"],
        ),
        ("secret", vec!["secret", "set", "X", "-R", "acme/other"]),
        (
            "workflow",
            vec!["workflow", "run", "ci.yml", "-R", "acme/other"],
        ),
    ] {
        let err = gh_err(&format!("ghg-family-{name}"), Some(WRITE_REPO), &args).await;
        assert!(
            matches!(err, ToolError::GhWriteDenied { .. }),
            "{name} must be gated: {err:?}"
        );
    }
}

#[tokio::test]
async fn read_subcommands_of_gated_families_pass() {
    gh_ok(
        "ghg-release-list",
        Some(WRITE_REPO),
        &["release", "list", "-R", "acme/other"],
    )
    .await;
    gh_ok(
        "ghg-repo-view",
        Some(WRITE_REPO),
        &["repo", "view", "acme/other"],
    )
    .await;
    gh_ok(
        "ghg-secret-list",
        Some(WRITE_REPO),
        &["secret", "list", "-R", "acme/other"],
    )
    .await;
    gh_ok(
        "ghg-run-list",
        Some(WRITE_REPO),
        &["run", "list", "-R", "acme/other"],
    )
    .await;
}

#[tokio::test]
async fn api_write_methods_are_gated_by_url_target() {
    let err = gh_err(
        "ghg-api-post",
        Some(WRITE_REPO),
        &["api", "repos/acme/other/pulls", "-X", "POST"],
    )
    .await;
    match &err {
        ToolError::GhWriteDenied { target } => assert_eq!(target, "acme/other"),
        other => panic!("expected GhWriteDenied, got {other:?}"),
    }
    gh_ok(
        "ghg-api-post-ok",
        Some(WRITE_REPO),
        &["api", "repos/acme/widgets/pulls", "-X", "POST"],
    )
    .await;
    gh_ok(
        "ghg-api-get",
        Some(WRITE_REPO),
        &["api", "repos/acme/other/pulls"],
    )
    .await;
}

#[tokio::test]
async fn api_body_flags_count_as_writes() {
    let err = gh_err(
        "ghg-api-field",
        Some(WRITE_REPO),
        &["api", "repos/acme/other/pulls", "-f", "a=b"],
    )
    .await;
    assert!(matches!(err, ToolError::GhWriteDenied { .. }), "{err:?}");
    let err = gh_err(
        "ghg-api-method-eq",
        Some(WRITE_REPO),
        &["api", "repos/acme/other/pulls", "--method=PATCH"],
    )
    .await;
    assert!(matches!(err, ToolError::GhWriteDenied { .. }), "{err:?}");
    let err = gh_err(
        "ghg-api-attached",
        Some(WRITE_REPO),
        &["api", "repos/acme/other/pulls", "-XDELETE"],
    )
    .await;
    assert!(matches!(err, ToolError::GhWriteDenied { .. }), "{err:?}");
}

#[tokio::test]
async fn api_write_without_a_repo_target_denies() {
    let err = gh_err(
        "ghg-api-user",
        Some(WRITE_REPO),
        &["api", "user", "-X", "POST"],
    )
    .await;
    assert!(matches!(err, ToolError::GhWriteUntargeted), "{err:?}");
}

#[tokio::test]
async fn setup_git_is_denied_while_the_knob_is_set() {
    let err = gh_err("ghg-setupgit", Some(WRITE_REPO), &["auth", "setup-git"]).await;
    assert!(matches!(err, ToolError::GhSetupGitDenied), "{err:?}");
    let text = err.to_string();
    assert!(text.contains("CRAFT_AGENT_WRITE_REPO"), "{text}");
    gh_ok("ghg-auth-status", Some(WRITE_REPO), &["auth", "status"]).await;
}

#[tokio::test]
async fn unset_knob_leaves_gh_ungated() {
    gh_ok(
        "ghg-unset-write",
        None,
        &["pr", "create", "-R", "acme/other"],
    )
    .await;
    gh_ok("ghg-unset-setup", None, &["auth", "setup-git"]).await;
}

#[tokio::test]
async fn malformed_knob_denies_writes_and_passes_reads() {
    let err = gh_err(
        "ghg-badknob",
        Some("acme"),
        &["pr", "create", "-R", "acme/widgets"],
    )
    .await;
    assert!(matches!(err, ToolError::GhWriteRepoMalformed), "{err:?}");
    gh_ok(
        "ghg-badknob-read",
        Some("acme"),
        &["pr", "view", "-R", "acme/other"],
    )
    .await;
}

#[tokio::test]
async fn repo_targets_match_case_insensitively() {
    gh_ok(
        "ghg-case",
        Some("Acme/Widgets"),
        &["pr", "create", "-R", "ACME/widgets"],
    )
    .await;
}

#[tokio::test]
async fn every_repo_flag_form_is_scanned() {
    let err = gh_err(
        "ghg-repo-eq",
        Some(WRITE_REPO),
        &["pr", "create", "--repo=acme/other"],
    )
    .await;
    assert!(matches!(err, ToolError::GhWriteDenied { .. }), "{err:?}");
    gh_ok(
        "ghg-long-flag",
        Some(WRITE_REPO),
        &["pr", "create", "--repo", "acme/widgets"],
    )
    .await;
    let err = gh_err("ghg-dangling", Some(WRITE_REPO), &["pr", "create", "-R"]).await;
    assert!(matches!(err, ToolError::GhWriteUntargeted), "{err:?}");
}

#[tokio::test]
async fn unknown_first_level_commands_stay_passable() {
    gh_ok(
        "ghg-browse",
        Some(WRITE_REPO),
        &["browse", "-R", "acme/other"],
    )
    .await;
    gh_ok("ghg-search", Some(WRITE_REPO), &["search", "code", "tok"]).await;
}
