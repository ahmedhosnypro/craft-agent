//! Live corpus for the judge seat repair loop: a real gateway drives a
// produce → gate workflow, and a wrapper forces the seat's first
// verdicts to be malformed the way a model can produce them — the loop
// must re-run the seat until the verdict parses, with a real verdict
// settling the gate. Runs via `just integration_workflow_judge`.
#![allow(clippy::unwrap_used)]

#[path = "../../craft-providers/tests/byok_env.rs"]
mod byok_env;

use std::collections::BTreeMap;
use std::sync::Mutex;

use craft_app::workflow_exec::WorkflowExecutor;
use craft_core::agents::{CatalogHit, load_catalog};
use craft_core::provider::Provider;
use craft_providers::openai::OpenAiProvider;
use craft_workflow::judge::parse_merged;
use craft_workflow::{
    Journal, RunConclusion, StepExecutor, StepInputs, StepOutcome, WorkflowError, WorkflowPlan,
    run_workflow,
};

fn test_workspace(label: &str) -> Result<std::path::PathBuf, String> {
    let path =
        std::env::temp_dir().join(format!("craft-agent-judge-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&path).map_err(|err| err.to_string())?;
    Ok(path)
}

type TestError = Box<dyn std::error::Error + Send + Sync>;

// The whole workflow — a producer run plus every seat attempt — under one
// bound; a stalled gateway is a broken corpus, not a hang. Generous on
// purpose: the BYOK endpoint serves thinking models whose turns run for
// minutes each, and this workflow runs several.
const LIVE_BOUND: std::time::Duration = std::time::Duration::from_secs(3600);

// The forced malformed verdicts: unquoted keys, the shape a model emits
// when it improvises the contract.
const FORCED_MALFORMED: &str = r#"{verdict: "pass", issues: []}"#;

/// Wraps the production executor: the first `force` attempts of one step
/// return the malformed verdict by construction, every later attempt
/// rides the real gateway.
struct ForcingExecutor<P: Provider + Send + Sync> {
    inner: WorkflowExecutor<P>,
    step: String,
    force: usize,
    calls: Mutex<BTreeMap<String, usize>>,
}

impl<P: Provider + Send + Sync> ForcingExecutor<P> {
    fn note(&self, id: &str) -> usize {
        let mut calls = self.calls.lock().unwrap();
        let entry = calls.entry(id.to_string()).or_insert(0);
        *entry += 1;
        *entry
    }
}

impl<P: Provider + Send + Sync> StepExecutor for ForcingExecutor<P> {
    async fn execute(&self, step: StepInputs) -> Result<StepOutcome, WorkflowError> {
        let attempt = self.note(&step.id);
        if step.id == self.step && attempt <= self.force {
            return Ok(StepOutcome {
                conclusion: craft_workflow::StepConclusion::Success,
                usage: craft_workflow::StepUsage::default(),
                final_message: String::from(FORCED_MALFORMED),
            });
        }
        self.inner.execute(step).await
    }
}

const PLAN: &str = r#"---
name: repair
steps:
  - id: produce
    agent: producer
    body: |
      Write the word ready into artifact.txt in the workspace root using
      the write_file tool. Final message: the path you wrote.
  - id: gate
    kind: judge
    agent: reviewer
    needs: [produce]
    judge:
      panel: 1
      merge: unanimous
      max-feedback: 0
    body: |
      Read artifact.txt in the workspace root. Rubric: the file exists
      and its content, trimmed, is exactly "ready". Return the verdict
      JSON: {"verdict":"pass"|"fail","issues":["..."]} — a pass carries
      an empty issues array.
---"#;

const PRODUCER_PROFILE: &str = r#"---
name: producer
description: Writes one small artifact for the live judge corpus.
promptMode: full
tools: write_file
maxTurns: 10
---
You are the producer. Do exactly what your prompt says, nothing else.
"#;

const REVIEWER_PROFILE: &str = r#"---
name: reviewer
description: One judge seat for the live seat-repair corpus.
promptMode: full
tools: read_file
maxTurns: 10
---
You are one judge seat. Your final message must be the verdict JSON,
exactly this shape and nothing else around it:
{"verdict":"pass"|"fail","issues":["..."]}
Judge only the rubric in your prompt.
"#;

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn a_seat_repairs_until_the_verdict_parses_over_the_live_gateway() -> Result<(), TestError> {
    let keys = crate::require_keys!("judge seat repair");
    let workspace = test_workspace("judge")?;
    let agents_dir = workspace.join(".craft-agent/agents");
    std::fs::create_dir_all(&agents_dir).map_err(|err| err.to_string())?;
    std::fs::write(agents_dir.join("producer.md"), PRODUCER_PROFILE)
        .map_err(|err| err.to_string())?;
    std::fs::write(agents_dir.join("reviewer.md"), REVIEWER_PROFILE)
        .map_err(|err| err.to_string())?;
    let sessions = workspace.join("sessions");
    std::fs::create_dir_all(&sessions).map_err(|err| err.to_string())?;

    let plan = WorkflowPlan::parse("repair.md", PLAN).map_err(|err| err.to_string())?;
    let catalog = load_catalog(&workspace).map_err(|err| err.to_string())?;
    let mut agents = BTreeMap::new();
    for spec in &plan.steps {
        let agent = match catalog.lookup(&spec.agent) {
            CatalogHit::Found(agent) => agent.clone(),
            other => {
                return Err(format!("agent `{}` did not load: {other:?}", spec.agent).into());
            }
        };
        agents.insert(spec.id.clone(), agent);
    }

    let base_url = keys.base_url.clone();
    let model = keys.model.clone();
    let api_key = keys.api_key.clone();
    let mount: craft_app::workflow_exec::StepMount<OpenAiProvider> = {
        let base_url = base_url.clone();
        let model = model.clone();
        let api_key = api_key.clone();
        std::sync::Arc::new(move |_model: &str| {
            Ok(std::sync::Arc::new(
                OpenAiProvider::new(&base_url, &model, &api_key).map_err(|err| err.to_string())?,
            ))
        })
    };
    let inner = WorkflowExecutor::new(
        mount,
        model.clone(),
        workspace.clone(),
        sessions,
        Some(2_000_000),
        None,
        None::<u32>,
        agents,
        craft_runtime::ContextKnobs::default(),
    );
    // Two forced malformed attempts: the seat must repair past both.
    let forcing = ForcingExecutor {
        inner,
        step: String::from("gate"),
        force: 2,
        calls: Mutex::new(BTreeMap::new()),
    };

    let journal_path = workspace.join("journal.jsonl");
    let mut journal = Journal::open(&journal_path).map_err(|err| err.to_string())?;
    let args = BTreeMap::new();
    let report = tokio::time::timeout(
        LIVE_BOUND,
        run_workflow(&plan, &args, &forcing, &mut journal),
    )
    .await
    .map_err(|_| String::from("the live judge workflow exceeded the 600s bound"))?
    .map_err(|err| err.to_string())?;

    assert!(
        matches!(report.conclusion, RunConclusion::Completed),
        "the workflow settles completed, got {:?}",
        report.conclusion
    );
    let gate_calls = forcing
        .calls
        .lock()
        .unwrap()
        .get("gate")
        .copied()
        .unwrap_or(0);
    assert_eq!(
        gate_calls, 3,
        "two forced attempts, then the real model's verdict"
    );
    let settled = report
        .steps
        .iter()
        .find(|step| step.id == "gate")
        .ok_or("the gate settled no row")?;
    let message = match &settled.status {
        craft_workflow::StepStatus::Settled { final_message, .. } => final_message.clone(),
        other => format!("{other:?}"),
    };
    let verdict = parse_merged(&message).map_err(|err| err.to_string())?;
    assert!(
        verdict.pass,
        "the real model passes the trivial rubric: {message}"
    );
    println!(
        "seat repair live: {} gate attempts, verdict {}",
        gate_calls, message
    );
    Ok(())
}
