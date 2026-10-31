//! RED corpus: the workflow step executor — ExitStatus→StepConclusion,
//! Usage→StepUsage, final-message capture, per-step knobs, and the
//! typed launch errors over the scripted-provider seam.

mod workflow_common;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use craft_app::workflow_exec::{StepMount, WorkflowExecutor, map_conclusion, map_usage};
use craft_core::error::ProviderError;
use craft_runtime::ExitStatus;
use craft_workflow::status::StepConclusion;
use craft_workflow::{StepExecutor, StepInputs, WorkflowError};

use workflow_common::{
    FakeProvider, Reply, reasoning_usage, scratch, step_agent_profile, text_reply, tool_reply,
    usage,
};

struct Rig {
    workspace: PathBuf,
}

fn rig(name: &str) -> std::io::Result<Rig> {
    let workspace = scratch(name)?;
    std::fs::create_dir_all(workspace.join("sessions"))?;
    Ok(Rig { workspace })
}

fn inputs(id: &str, max_turns: u32) -> StepInputs {
    StepInputs {
        id: id.to_string(),
        agent: workflow_common::AGENT_NAME.to_string(),
        model: None,
        max_turns,
        seat: None,
        feedback: Vec::new(),
        deny_read: Vec::new(),
        deny_write: Vec::new(),
        prompt: "Run the step.".to_string(),
    }
}

fn executor_with(
    provider: Arc<FakeProvider>,
    token_cap: u64,
) -> Result<WorkflowExecutor<FakeProvider>, std::io::Error> {
    let rig = rig("exec-bare")?;
    Ok(executor_over(&rig, provider, token_cap, BTreeMap::new()))
}

fn executor_over(
    rig: &Rig,
    provider: Arc<FakeProvider>,
    token_cap: u64,
    agents: BTreeMap<String, craft_core::agents::AgentFile>,
) -> WorkflowExecutor<FakeProvider> {
    executor_over_knobs(
        rig,
        provider,
        token_cap,
        agents,
        craft_runtime::ContextKnobs::default(),
    )
}

fn executor_over_knobs(
    rig: &Rig,
    provider: Arc<FakeProvider>,
    token_cap: u64,
    agents: BTreeMap<String, craft_core::agents::AgentFile>,
    context: craft_runtime::ContextKnobs,
) -> WorkflowExecutor<FakeProvider> {
    let mount: StepMount<FakeProvider> = Arc::new(move |_model| Ok(provider.clone()));
    WorkflowExecutor::new(
        mount,
        "base-model".to_string(),
        rig.workspace.clone(),
        rig.workspace.join("sessions"),
        Some(token_cap),
        None,
        Some(3),
        agents,
        context,
    )
    .with_step_patience(Some(Duration::ZERO))
}

fn agent_map() -> BTreeMap<String, craft_core::agents::AgentFile> {
    let mut agents = BTreeMap::new();
    agents.insert("s1".to_string(), step_agent_profile());
    agents
}

async fn tool_step_outcome(
    tool_args: &'static str,
    max_turns: u32,
) -> Result<craft_workflow::StepOutcome, Box<dyn std::error::Error>> {
    let provider = FakeProvider::new(vec![tool_reply("c1", "read_file", tool_args, usage(1, 1))]);
    let rig = rig("exec")?;
    let executor = executor_over(&rig, provider, 4_000_000, agent_map());
    executor
        .execute(inputs("s1", max_turns))
        .await
        .map_err(Into::into)
}

#[tokio::test]
async fn success_maps_conclusion_usage_and_final_message() {
    let provider = FakeProvider::new(vec![text_reply("all done", usage(10, 5))]);
    let executor = executor_over(
        &rig("exec").expect("rig"),
        provider.clone(),
        4_000_000,
        agent_map(),
    );
    let outcome = executor.execute(inputs("s1", 25)).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);
    assert_eq!(outcome.final_message, "all done");
    assert_eq!(outcome.usage.turns, 1);
    assert_eq!(outcome.usage.input_tokens, 10);
    assert_eq!(outcome.usage.output_tokens, 5);
    assert_eq!(outcome.usage.reasoning_tokens, 0);
}

#[tokio::test]
async fn an_empty_model_turn_settles_as_error() {
    let provider = FakeProvider::new(vec![text_reply("", usage(1, 1))]);
    let executor = executor_over(&rig("exec").expect("rig"), provider, 4_000_000, agent_map());
    let outcome = executor.execute(inputs("s1", 25)).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Error);
    assert!(
        outcome.final_message.contains("empty"),
        "the empty-response detail becomes the preview: {}",
        outcome.final_message
    );
}

#[tokio::test]
async fn reasoning_tokens_map_to_their_own_bucket() {
    let provider = FakeProvider::new(vec![text_reply("ok", reasoning_usage(10, 5, 3))]);
    let executor = executor_over(&rig("exec").expect("rig"), provider, 4_000_000, agent_map());
    let outcome = executor.execute(inputs("s1", 25)).await.expect("step runs");
    assert_eq!(outcome.usage.reasoning_tokens, 3);
    assert_eq!(outcome.usage.total_tokens(), 18);
}

#[tokio::test]
async fn provider_failure_settles_as_error_with_the_detail() {
    let provider = FakeProvider::new(vec![Reply::Fail(ProviderError::Auth {
        detail: "bad key".to_string(),
    })]);
    let executor = executor_over(&rig("exec").expect("rig"), provider, 4_000_000, agent_map());
    let outcome = executor.execute(inputs("s1", 25)).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Error);
    assert_eq!(outcome.conclusion.exit_code(), 1);
    assert!(
        outcome.final_message.contains("bad key"),
        "the failure detail becomes the preview: {}",
        outcome.final_message
    );
}

#[tokio::test]
async fn turn_ceiling_settles_as_max_turns() {
    let outcome = tool_step_outcome("{\"path\": \"missing.txt\"}", 1)
        .await
        .expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::MaxTurns);
    assert_eq!(outcome.conclusion.exit_code(), 2);
}

#[tokio::test]
async fn token_cap_settles_as_budget() {
    let provider = FakeProvider::new(vec![text_reply("halfway", usage(1000, 10))]);
    let executor = executor_over(
        &rig("exec-budget").expect("rig"),
        provider,
        500,
        agent_map(),
    );
    let outcome = executor.execute(inputs("s1", 25)).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Budget);
    assert_eq!(outcome.conclusion.exit_code(), 3);
}

#[tokio::test]
async fn a_refused_path_repeats_to_stationarity_not_denial() {
    let outcome = tool_step_outcome("{\"path\": \".env\"}", 25)
        .await
        .expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Stationarity);
    assert_eq!(outcome.conclusion.exit_code(), 2);
}

#[tokio::test]
async fn identical_calls_settle_as_stationarity() {
    let outcome = tool_step_outcome("{\"path\": \"missing.txt\"}", 25)
        .await
        .expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Stationarity);
    assert_eq!(outcome.conclusion.exit_code(), 2);
}

#[tokio::test]
async fn retried_attempt_voids_the_partial_final_message() {
    let provider = FakeProvider::new(vec![
        workflow_common::dead_partial_reply("void-me"),
        text_reply("kept", usage(1, 1)),
    ]);
    let executor = executor_over(&rig("exec").expect("rig"), provider, 4_000_000, agent_map());
    let outcome = executor.execute(inputs("s1", 25)).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);
    assert_eq!(outcome.final_message, "kept");
}

#[tokio::test]
async fn missing_profile_is_a_typed_launch_error() {
    let provider = FakeProvider::new(vec![text_reply("unused", usage(1, 1))]);
    let executor = executor_with(provider, 4_000_000).expect("executor");
    let err = executor
        .execute(inputs("s1", 25))
        .await
        .expect_err("no resolved profile must fail");
    match err {
        WorkflowError::StepLaunch { step, .. } => assert_eq!(step, "s1"),
        other => panic!("expected a step launch error, got {other:?}"),
    }
}

#[tokio::test]
async fn session_transcript_lands_in_the_run_dir() {
    let rig = rig("exec-session").expect("rig");
    let provider = FakeProvider::new(vec![text_reply("done", usage(1, 1))]);
    let executor = executor_over(&rig, provider, 4_000_000, agent_map());
    executor.execute(inputs("s1", 25)).await.expect("step runs");
    assert!(
        rig.workspace.join("sessions").join("s1.jsonl").is_file(),
        "the step transcript must land beside the journal"
    );
}

#[tokio::test]
async fn step_model_wins_over_the_default_mount() {
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mount_seen = seen.clone();
    let mount: StepMount<FakeProvider> = Arc::new(move |model| {
        mount_seen
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(model.to_string());
        Ok(FakeProvider::new(vec![text_reply("ok", usage(1, 1))]))
    });
    let workspace_root = scratch("exec-models").expect("scratch dir");
    let executor = WorkflowExecutor::new(
        mount,
        "base-model".to_string(),
        workspace_root,
        PathBuf::from("sessions"),
        Some(4_000_000),
        None,
        Some(3),
        agent_map(),
        craft_runtime::ContextKnobs::default(),
    );
    let mut step = inputs("s1", 25);
    step.model = Some("glm-x".to_string());
    executor.execute(step).await.expect("step runs");
    let unnamed = executor.execute(inputs("s1", 25)).await;
    assert!(unnamed.is_ok(), "the default model mounts too");
    let seen = seen.lock().unwrap_or_else(|held| held.into_inner()).clone();
    assert_eq!(seen, vec!["glm-x".to_string(), "base-model".to_string()]);
}

#[test]
fn exit_status_maps_one_to_one_onto_conclusions() {
    assert_eq!(map_conclusion(ExitStatus::Ok), StepConclusion::Success);
    assert_eq!(map_conclusion(ExitStatus::Error), StepConclusion::Error);
    assert_eq!(
        map_conclusion(ExitStatus::MaxTurns),
        StepConclusion::MaxTurns
    );
    assert_eq!(
        map_conclusion(ExitStatus::Stationarity),
        StepConclusion::Stationarity
    );
    assert_eq!(map_conclusion(ExitStatus::Budget), StepConclusion::Budget);
    for exit in [
        ExitStatus::Ok,
        ExitStatus::Error,
        ExitStatus::MaxTurns,
        ExitStatus::Stationarity,
        ExitStatus::Budget,
    ] {
        assert_eq!(map_conclusion(exit).exit_code(), exit.code());
    }
}

#[test]
fn usage_mapping_keeps_the_turn_and_token_buckets() {
    let mapped = map_usage(7, &reasoning_usage(100, 40, 10));
    assert_eq!(mapped.turns, 7);
    assert_eq!(mapped.input_tokens, 100);
    assert_eq!(mapped.output_tokens, 40);
    assert_eq!(mapped.reasoning_tokens, 10);
    assert_eq!(mapped.total_tokens(), 150);
}
