//! The TDD-isolation end-to-end row: a three-step workflow — test
//! writer writes failing tests, a blind implementer profile is denied
//! reading `tests/**` at BOTH the profile and the step level, and a
//! judge gate settles the pair. Over the fake provider: the blind step
//! runs with a registry whose read of the test file fails closed, the
//! scripted tool call in the transcript proves it, and the judge
//! verdict settles the run.
#![allow(clippy::expect_used)]

mod workflow_common;

use std::fs;

use craft_app::workflow_cmd::{run_or_resume, workflow_run_dir};
use craft_workflow::plan::ArgValue;
use craft_workflow::run::{args_digest, run_id_for};
use craft_workflow::{Journal, WorkflowPlan};

use std::collections::BTreeMap;
use std::path::Path;
use workflow_common::{FakeProvider, Reply, repo, text_reply, usage};

const TDD_PLAN: &str = r#"---
name: tdd-isolation
steps:
  - id: write-tests
    agent: spec-writer
    max-turns: 3
    retry: 0
    on-failure: stop
    body: |
      Write the failing test for the greeting contract.
  - id: implement
    agent: blind-implementer
    needs: [write-tests]
    deny-read:
      - tests/**
    max-turns: 3
    retry: 0
    on-failure: stop
    body: |
      Implement against the spec.
  - id: gate
    kind: judge
    agent: reviewer
    needs: [implement]
    judge:
      panel: 1
      merge: unanimous
      max-feedback: 1
    max-turns: 3
    body: |
      Judge the implementation.
---"#;

fn write_agents(workspace: &Path) -> std::io::Result<()> {
    let dir = workspace.join(".craft-agent").join("agents");
    fs::write(
        dir.join("spec-writer.md"),
        r#"---
name: spec-writer
description: >-
  Use when a TDD pipeline needs the failing tests written from a spec.
  Writes the test corpus only.
promptMode: full
tools: read_file, write_file, bash
denyWrite: src/**
---
You write failing tests from the specification you are handed.
"#,
    )?;
    fs::write(
        dir.join("blind-implementer.md"),
        r#"---
name: blind-implementer
description: >-
  Use when a TDD pipeline needs the implementation written from a spec
  with the test corpus hidden so the contract cannot be cheated.
promptMode: full
tools: read_file, write_file, edit_file, glob, grep, bash
denyRead: tests/**
---
You implement against the specification. You cannot read the test
corpus; the spec is the contract.
"#,
    )?;
    fs::write(
        dir.join("reviewer.md"),
        r#"---
name: reviewer
description: >-
  Use when a TDD pipeline needs a judge that runs the suite and reports
  a pass/fail verdict. Read-only otherwise.
promptMode: full
tools: read_file, glob, grep, bash
---
You run the test suite and answer with a verdict JSON object.
"#,
    )?;
    Ok(())
}

fn rig(name: &str) -> Result<(std::path::PathBuf, WorkflowPlan), Box<dyn std::error::Error>> {
    let workspace = repo(name)?;
    write_agents(&workspace)?;
    fs::create_dir_all(workspace.join("tests"))?;
    fs::write(
        workspace.join("tests").join("greeting_test.py"),
        "def test_greeting():\n    assert greet('world') == 'hello, world'\n",
    )?;
    let path = workspace.join("plan.md");
    fs::write(&path, TDD_PLAN)?;
    Ok((workspace, WorkflowPlan::parse_file(&path)?))
}

fn executor(
    workspace: &std::path::Path,
    run_dir: &Path,
    plan: &WorkflowPlan,
    provider: std::sync::Arc<FakeProvider>,
) -> Result<craft_app::workflow_exec::WorkflowExecutor<FakeProvider>, craft_app::AppError> {
    let agents = craft_app::workflow_cmd::resolve_step_agents(plan, workspace)?;
    let mount: craft_app::workflow_exec::StepMount<FakeProvider> =
        std::sync::Arc::new(move |_model| Ok(provider.clone()));
    Ok(craft_app::workflow_exec::WorkflowExecutor::new(
        mount,
        "base-model".to_string(),
        workspace.to_path_buf(),
        run_dir.join("sessions"),
        Some(plan.limits.max_total_tokens),
        None,
        Some(3),
        agents,
        craft_runtime::ContextKnobs::default(),
    ))
}

/// The blind step's first turn: the model tries to read the hidden
/// test file (the scope denial comes back as its tool result), then it
/// finishes with its plain final message.
fn blind_reply() -> Vec<Reply> {
    vec![
        workflow_common::tool_reply(
            "call-1",
            "read_file",
            r#"{"path":"tests/greeting_test.py"}"#,
            usage(20, 2),
        ),
        text_reply("implemented blind", usage(20, 4)),
    ]
}

#[tokio::test]
async fn the_blind_implementer_cannot_read_the_test_corpus() {
    let (workspace, plan) = rig("tdd").expect("rig");
    let args: BTreeMap<String, ArgValue> = BTreeMap::new();
    let digest = args_digest(&args).expect("digest");
    let run_id = run_id_for(&plan.plan_hash, &digest);
    let run_dir = workflow_run_dir(&workspace, &run_id);
    fs::create_dir_all(run_dir.join("sessions")).expect("run dir");
    let mut replies = vec![text_reply("tests written", usage(10, 2))];
    replies.extend(blind_reply());
    replies.push(text_reply(
        r#"{"verdict":"pass","issues":[]}"#,
        usage(10, 2),
    ));
    let provider = FakeProvider::new(replies);
    let mut journal = Journal::open(&run_dir.join("journal.jsonl")).expect("journal");
    let report = run_or_resume(
        &plan,
        &args,
        &executor(&workspace, &run_dir, &plan, provider.clone()).expect("executor"),
        &mut journal,
    )
    .await
    .expect("the workflow runs");
    assert_eq!(report.exit_code, 0, "{report:?}");
    assert!(report.is_success());
    assert_eq!(report.steps.len(), 3);
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        4,
        "one model call per step, the blind step two"
    );
    let tools: Vec<&craft_core::types::ChatMessage> = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter(|message| {
            matches!(
                message.as_ref(),
                craft_core::types::ChatMessage::Tool { .. }
            )
        })
        .map(|message| message.as_ref())
        .collect();
    assert_eq!(tools.len(), 1, "only the blind step made a tool call");
    let Some(craft_core::types::ChatMessage::Tool {
        content, is_error, ..
    }) = tools.first().cloned()
    else {
        panic!("the tool message shape changed");
    };
    assert!(is_error, "the hidden read fails closed: {content}");
    assert!(
        content.contains("denied by scope glob") && content.contains("tests/**"),
        "the denial names the scope glob: {content}"
    );
    assert!(
        !content.contains("def test_"),
        "the hidden test body never reaches the model: {content}"
    );
}

const PROFILE_ONLY_PLAN: &str = r#"---
name: tdd-profile-deny
steps:
  - id: implement
    agent: blind-implementer
    max-turns: 3
    retry: 0
    on-failure: stop
    body: |
      Implement against the spec; the profile hides the test corpus.
  - id: gate
    kind: judge
    agent: reviewer
    needs: [implement]
    judge:
      panel: 1
      merge: unanimous
      max-feedback: 1
    max-turns: 3
    body: |
      Judge the implementation.
---"#;

#[tokio::test]
async fn a_profile_deny_alone_hides_the_test_corpus() {
    let (workspace, _) = rig("tdd-profile").expect("rig");
    let path = workspace.join("plan.md");
    fs::write(&path, PROFILE_ONLY_PLAN).expect("plan");
    let plan = WorkflowPlan::parse_file(&path).expect("parsed");
    let args: BTreeMap<String, ArgValue> = BTreeMap::new();
    let digest = args_digest(&args).expect("digest");
    let run_id = run_id_for(&plan.plan_hash, &digest);
    let run_dir = workflow_run_dir(&workspace, &run_id);
    fs::create_dir_all(run_dir.join("sessions")).expect("run dir");
    let mut replies = blind_reply();
    replies.push(text_reply(
        r#"{"verdict":"pass","issues":[]}"#,
        usage(10, 2),
    ));
    let provider = FakeProvider::new(replies);
    let mut journal = Journal::open(&run_dir.join("journal.jsonl")).expect("journal");
    let report = run_or_resume(
        &plan,
        &args,
        &executor(&workspace, &run_dir, &plan, provider.clone()).expect("executor"),
        &mut journal,
    )
    .await
    .expect("the workflow runs");
    assert_eq!(report.exit_code, 0, "{report:?}");
    let requests = provider.requests();
    let tools: Vec<&craft_core::types::ChatMessage> = requests
        .iter()
        .flat_map(|request| request.messages.iter())
        .filter(|message| {
            matches!(
                message.as_ref(),
                craft_core::types::ChatMessage::Tool { .. }
            )
        })
        .map(|message| message.as_ref())
        .collect();
    assert_eq!(tools.len(), 1);
    let Some(craft_core::types::ChatMessage::Tool {
        content, is_error, ..
    }) = tools.first().cloned()
    else {
        panic!("the tool message shape changed");
    };
    assert!(
        is_error,
        "the profile deny fails closed with no step block: {content}"
    );
    assert!(content.contains("tests/**"), "{content}");
    assert!(!content.contains("def test_"), "{content}");
}
