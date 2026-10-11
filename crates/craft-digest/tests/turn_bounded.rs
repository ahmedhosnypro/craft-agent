// The digest turn is fail-closed: a provider that never finishes aborts
// with a typed error carrying the cancellation category, and no digest
// file is ever written for a failed turn.
// The std script lock is held across the pipeline await on purpose: it
// serializes whole tests on single-threaded test runtimes.
#![allow(clippy::unwrap_used)]
#![allow(clippy::await_holding_lock)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, Provider, boxed};
use craft_core::types::{ModelRequest, StopReason, Usage};
use craft_digest::clone_runner::CloneRunner;
use craft_digest::collect::GhRunner;
use craft_digest::error::DigestError;
use craft_digest::jscpd_runner::JscpdRunner;
use craft_digest::pipeline::{DigestDeps, DigestOptions, DigestOutcome, run_digest};
use futures::stream;
use std::time::Duration;

type ScriptedTurn = Vec<Result<ModelEvent, ProviderError>>;

/// Fake-runner scripts are exec'd right after being written; a parallel
/// fork in another test can inherit the still-open write fd and fail the
/// exec with ETXTBSY, so the script-driving tests serialize.
static SCRIPT_LOCK: Mutex<()> = Mutex::new(());

fn script_guard() -> MutexGuard<'static, ()> {
    SCRIPT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn zero_usage() -> Usage {
    Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

fn tool_turn(id: &str, name: &str, args: &str) -> ScriptedTurn {
    vec![
        Ok(ModelEvent::ToolCallDelta {
            index: 0,
            id: Some(String::from(id)),
            name: Some(String::from(name)),
            arguments: String::from(args),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: zero_usage(),
        }),
    ]
}

/// Repeats its single scripted turn forever.
struct LoopingProvider {
    turn: Mutex<ScriptedTurn>,
}

impl Provider for LoopingProvider {
    async fn stream(&self, _request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        let turn = self
            .turn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        Ok(boxed(stream::iter(turn)))
    }
}

fn looping(turn: ScriptedTurn) -> Arc<LoopingProvider> {
    Arc::new(LoopingProvider {
        turn: Mutex::new(turn),
    })
}

fn fixture(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/craft-digest-corpus")
        .join(format!("turn-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("bin")).unwrap();
    let prompt = dir.join("ws").join("config");
    fs::create_dir_all(&prompt).unwrap();
    fs::write(
        prompt.join("digest-prompt.md"),
        "Digest rules: measured facts only.",
    )
    .unwrap();
    dir
}

fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir
        .join("bin")
        .join(format!("{}-{}", body.len(), std::process::id()));
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn deps(root: &Path) -> DigestDeps {
    let gh = GhRunner::with_program(script(root, "echo '[]'"));
    let clone_body = "mkdir -p \"$5/src\"\nprintf 'text\n' > \"$5/AGENTS.md\"";
    let clone = CloneRunner::with_program(script(root, clone_body));
    let jscpd_body = "out=\"\"\nprev=\"\"\nfor a in \"$@\"; do\n  if [ \"$prev\" = \"--output\" ]; then out=\"$a\"; fi\n  prev=\"$a\"\ndone\nmkdir -p \"$out\"\nprintf '{\"duplicates\":[]}' > \"$out/jscpd-report.json\"";
    let jscpd = JscpdRunner::with_program(script(root, jscpd_body));
    DigestDeps {
        gh,
        clone_runner: clone,
        jscpd,
    }
}

fn options(root: &Path, max_turns: u32) -> DigestOptions {
    DigestOptions {
        org: String::from("storytold"),
        repos: vec![String::from("alpha")],
        lookback: Duration::from_secs(24 * 3600),
        dry_run: false,
        workspace_root: root.join("ws"),
        prompt_path: None,
        max_turns,
        max_total_tokens: 4_000_000,
        idle_timeout: None,
    }
}

async fn run_bounded(
    root: &Path,
    turn: ScriptedTurn,
    max_turns: u32,
) -> Result<DigestOutcome, DigestError> {
    let mut sink = Vec::new();
    run_digest(
        looping(turn),
        &deps(root),
        &options(root, max_turns),
        &mut sink,
    )
    .await
}

fn turn_category(error: DigestError) -> (String, String) {
    match error {
        DigestError::Turn { category, detail } => (category, detail),
        other => (String::new(), format!("expected a Turn error, got {other}")),
    }
}

fn assert_category(error: DigestError, expected: &str) {
    let (category, detail) = turn_category(error);
    assert_eq!(category, expected, "detail: {detail}");
}

#[tokio::test]
async fn max_turns_abort_maps_to_a_typed_error_and_writes_nothing() {
    let _guard = script_guard();
    let root = fixture("max-turns");
    // bash is outside the digest posture: each call must come back as an
    // is_error tool result, never as an exit-4 denial, so the run spends
    // its whole turn budget and fails as max-turns.
    let error = run_bounded(&root, tool_turn("call-1", "bash", "{}"), 2)
        .await
        .unwrap_err();
    assert_category(error, "max_turns");
    assert!(!root.join("ws/out/digest.md").exists());
    assert!(!root.join("ws/.craft-agent").exists());
}

#[tokio::test]
async fn stationarity_abort_maps_to_a_typed_error() {
    let _guard = script_guard();
    let root = fixture("stationarity");
    let error = run_bounded(
        &root,
        tool_turn("call-1", "read_file", r#"{"path":"src/lib.rs"}"#),
        40,
    )
    .await
    .unwrap_err();
    assert_category(error, "stationarity");
    assert!(!root.join("ws/out/digest.md").exists());
    assert!(!root.join("ws/.craft-agent").exists());
}

#[tokio::test]
async fn sensitive_path_denial_maps_to_a_typed_error() {
    let _guard = script_guard();
    let root = fixture("denial");
    let error = run_bounded(
        &root,
        tool_turn("call-1", "read_file", r#"{"path":".env"}"#),
        40,
    )
    .await
    .unwrap_err();
    assert_category(error, "sensitive_path");
    assert!(!root.join("ws/out/digest.md").exists());
    assert!(!root.join("ws/.craft-agent").exists());
}
