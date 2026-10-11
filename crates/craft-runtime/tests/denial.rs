//! Denial corpus: a tool call on a banned path aborts the run with the
//! exit-4 family and its cancellation category.

mod common;

use common::*;
use craft_runtime::{CancelCategory, RunConfig, Runner};

fn config() -> RunConfig {
    RunConfig::new(std::path::PathBuf::from("."))
}

#[tokio::test]
async fn sensitive_path_denial_aborts_with_exit_four() {
    let provider = FakeProvider::new(vec![tool_turn(
        "c1",
        "read_file",
        r#"{"path":".env"}"#,
        usage(1, 1),
    )]);
    let runner = Runner::new(provider, builtin_registry(), config());
    let (outcome, events) = run_collect(&runner, "read the secrets").await;

    assert_eq!(outcome.exit_code(), 4);
    assert_eq!(outcome.category, CancelCategory::SensitivePath);
    assert!(outcome.detail.contains("sensitive"));
    assert!(matches!(
        events.last(),
        Some(craft_runtime::RuntimeEvent::RunFinished { exit_code, category, .. })
        if *exit_code == 4 && *category == CancelCategory::SensitivePath
    ));
}

#[tokio::test]
async fn workspace_escape_denial_aborts_with_exit_four() {
    let provider = FakeProvider::new(vec![tool_turn(
        "c1",
        "read_file",
        r#"{"path":"../outside.txt"}"#,
        usage(1, 1),
    )]);
    let runner = Runner::new(provider, builtin_registry(), config());
    let (outcome, _) = run_collect(&runner, "escape").await;

    assert_eq!(outcome.exit_code(), 4);
    assert_eq!(outcome.category, CancelCategory::EscapesWorkspace);
    assert!(outcome.detail.contains("workspace"));
}
