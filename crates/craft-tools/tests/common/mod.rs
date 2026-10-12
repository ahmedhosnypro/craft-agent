//! Shared fixtures for the craft-tools corpora: throwaway workspaces,
//! fast tool limits, event drivers, and the fake `gh` binary.

#![allow(dead_code)]
// Corpus fixtures: expect() is the loud-test idiom here.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::time::Duration;

use craft_tools::{ToolCtx, ToolError, ToolEvent, ToolLimits, ToolStream};

pub fn tmp_ws(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/craft-tools-corpus")
        .join(format!("{}-{}", name, process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("workspace create");
    fs::canonicalize(&dir).unwrap_or(dir)
}

pub fn write(ws: &Path, rel: &str, content: &str) {
    let path = ws.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent create");
    }
    fs::write(&path, content).expect("fixture write");
}

pub fn write_bytes(ws: &Path, rel: &str, content: &[u8]) {
    let path = ws.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent create");
    }
    fs::write(&path, content).expect("fixture write");
}

pub fn fast_limits() -> ToolLimits {
    ToolLimits {
        bash_default_timeout: Duration::from_millis(1200),
        bash_max_timeout: Duration::from_secs(4),
        proc_grace: Duration::from_millis(600),
        proc_out_cap: 4096,
        gh_timeout: Duration::from_secs(8),
        read_cap: 1024,
        write_cap: 1024,
        max_path_bytes: 4096,
        max_command_bytes: 4096,
        max_pattern_bytes: 256,
        max_args: 8,
        glob_max_results: 6,
        grep_max_results: 5,
        walk_max_entries: 500,
        walk_max_depth: 16,
        grep_file_cap: 1024,
        grep_line_cap: 64,
    }
}

pub fn ctx_for(ws: &Path) -> ToolCtx {
    ToolCtx::new(ws.to_path_buf())
}

pub fn ctx_env(ws: &Path, pairs: Vec<(String, String)>) -> ToolCtx {
    ToolCtx::with_env(ws.to_path_buf(), pairs)
}

pub async fn run(stream: ToolStream) -> Vec<Result<ToolEvent, ToolError>> {
    use futures::StreamExt;
    stream.collect().await
}

/// The single terminal event, or panics with the actual error.
pub fn terminal(events: &[Result<ToolEvent, ToolError>]) -> String {
    let terminal = events.iter().find_map(|e| match e {
        Ok(ToolEvent::Terminal(t)) => Some(t.output.clone()),
        _ => None,
    });
    match terminal {
        Some(out) => out,
        None => panic!("no terminal event in {:?}", events),
    }
}

/// The first typed error, or panics if the run succeeded.
pub fn error(events: &[Result<ToolEvent, ToolError>]) -> ToolError {
    let err = events.iter().find_map(|e| match e {
        Err(e) => Some(e.clone()),
        _ => None,
    });
    match err {
        Some(e) => e,
        None => panic!("no error event in {:?}", events),
    }
}

/// Progress payloads in arrival order.
pub fn progress(events: &[Result<ToolEvent, ToolError>]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Ok(ToolEvent::Progress(p)) => Some(p.text.clone()),
            _ => None,
        })
        .collect()
}

/// Exactly one terminal and no errors, else panic.
pub fn assert_clean(events: &[Result<ToolEvent, ToolError>]) {
    let terminals = events
        .iter()
        .filter(|e| matches!(e, Ok(ToolEvent::Terminal(_))))
        .count();
    let errors = events.iter().filter(|e| e.is_err()).count();
    assert_eq!(terminals, 1, "expected one terminal: {events:?}");
    assert_eq!(errors, 0, "expected no errors: {events:?}");
}

/// A fake `gh` on PATH, driven by FAKE_GH_MODE from the (scrubbed) child env.
pub fn fake_gh(dir: &Path, body: &str) -> PathBuf {
    let script = format!("#!/bin/sh\n{body}\n");
    let path = dir.join("gh");
    fs::write(&path, script).expect("fake gh write");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("fake gh chmod");
    fs::File::open(&path)
        .and_then(|f| f.sync_all())
        .expect("fake gh sync");
    dir.to_path_buf()
}

/// Explicit env for the child: fake keys to prove scrubbing, plus PATH.
pub fn fake_child_env(extra_path: Option<PathBuf>) -> Vec<(String, String)> {
    let path = match extra_path {
        Some(p) => format!(
            "{}:{}",
            p.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
        None => std::env::var("PATH").unwrap_or_default(),
    };
    vec![
        ("PATH".to_string(), path),
        ("HOME".to_string(), "/tmp".to_string()),
        ("OPENAI_API_KEY".to_string(), "sk-fake-openai".to_string()),
        (
            "ANTHROPIC_API_KEY".to_string(),
            "sk-fake-anthropic".to_string(),
        ),
        ("GOOGLE_API_KEY".to_string(), "fake-google".to_string()),
        ("STRIPE_API_KEY".to_string(), "sk-fake-stripe".to_string()),
        ("API_KEY".to_string(), "bare-fake".to_string()),
        ("TEST_BYOK_API_KEY".to_string(), "fake-byok".to_string()),
        ("TEST_BYOK_BASE_URL".to_string(), "http://fake".to_string()),
        ("FAKE_GH_MODE".to_string(), "ok".to_string()),
        ("GH_TOKEN".to_string(), "ghp-fake-token".to_string()),
    ]
}
