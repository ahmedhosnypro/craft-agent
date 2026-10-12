//! Shared test fixtures: a scripted fake executor and temp-dir helpers.
#![allow(dead_code)]
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use craft_workflow::{
    StepConclusion, StepExecutor, StepInputs, StepOutcome, StepUsage, WorkflowError,
};

pub fn usage(turns: u64, input: u64, output: u64, reasoning: u64) -> StepUsage {
    StepUsage {
        turns,
        input_tokens: input,
        output_tokens: output,
        reasoning_tokens: reasoning,
    }
}

/// The default scripted outcome: 2 turns, 100 tokens, a one-line message.
pub fn outcome_ok(message: &str) -> StepOutcome {
    StepOutcome {
        conclusion: StepConclusion::Success,
        usage: usage(2, 60, 30, 10),
        final_message: message.to_string(),
    }
}

pub fn outcome_fail(conclusion: StepConclusion, message: &str) -> StepOutcome {
    StepOutcome {
        conclusion,
        usage: usage(2, 60, 30, 10),
        final_message: message.to_string(),
    }
}

pub fn no_args() -> BTreeMap<String, craft_workflow::ArgValue> {
    BTreeMap::new()
}

/// Serves scripted outcomes per step id (per attempt, FIFO); ids without a
/// script or an exhausted queue succeed. Also records prompts, call order,
/// live concurrency, and can fail a launch outright.
#[derive(Default)]
pub struct FakeExecutor {
    script: Mutex<BTreeMap<String, VecDeque<StepOutcome>>>,
    launch_errors: Mutex<BTreeMap<String, String>>,
    calls: Mutex<Vec<String>>,
    prompts: Mutex<BTreeMap<String, Vec<String>>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl FakeExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue(&self, id: &str, outcome: StepOutcome) {
        self.script
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_default()
            .push_back(outcome);
    }

    pub fn queue_ok(&self, id: &str, message: &str) {
        self.queue(id, outcome_ok(message));
    }

    pub fn queue_fail(&self, id: &str, conclusion: StepConclusion) {
        self.queue(id, outcome_fail(conclusion, &format!("failed {id}")));
    }

    pub fn launch_error(&self, id: &str, detail: &str) {
        self.launch_errors
            .lock()
            .unwrap()
            .insert(id.to_string(), detail.to_string());
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn prompts_for(&self, id: &str) -> Vec<String> {
        self.prompts
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight.load(Ordering::SeqCst)
    }
}

impl StepExecutor for FakeExecutor {
    async fn execute(&self, step: StepInputs) -> Result<StepOutcome, WorkflowError> {
        let cur = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(cur, Ordering::SeqCst);
        self.calls.lock().unwrap().push(step.id.clone());
        self.prompts
            .lock()
            .unwrap()
            .entry(step.id.clone())
            .or_default()
            .push(step.prompt.clone());
        tokio::task::yield_now().await;
        let result = match self.launch_errors.lock().unwrap().get(&step.id) {
            Some(detail) => Err(WorkflowError::JournalIo {
                path: step.id.clone(),
                detail: detail.clone(),
            }),
            None => {
                let mut script = self.script.lock().unwrap();
                match script.get_mut(&step.id) {
                    Some(queue) if !queue.is_empty() => Ok(queue.pop_front().unwrap()),
                    _ => Ok(outcome_ok(&format!("did {}", step.id))),
                }
            }
        };
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

pub fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("craft-wf-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn journal_path(dir: &Path) -> PathBuf {
    dir.join("workflow.jsonl")
}

pub mod plan_texts {
    pub const CHAIN: &str = r#"---
name: chain
steps:
  - id: a
    agent: worker
    body: |
      do a
  - id: b
    agent: worker
    needs: [a]
    body: |
      do b
  - id: c
    agent: worker
    needs: [b]
    body: |
      do c
---

# Shared rules

- be terse
"#;

    pub const FOUR_INDEPENDENT: &str = r#"---
name: wide
limits:
  max-steps: 4
steps:
  - id: w
    agent: worker
    body: |
      w
  - id: x
    agent: worker
    body: |
      x
  - id: y
    agent: worker
    body: |
      y
  - id: z
    agent: worker
    body: |
      z
---

prelude
"#;

    pub const DIAMOND: &str = r#"---
name: diamond
steps:
  - id: top
    agent: worker
    body: |
      top
  - id: left
    agent: worker
    needs: [top]
    body: |
      left
  - id: right
    agent: worker
    needs: [top]
    body: |
      right
  - id: join
    agent: worker
    needs: [left, right]
    body: |
      join
---

prelude
"#;

    pub const ARGS_PLAN: &str = r#"---
name: argplan
args:
  window: { type: string, default: 24h }
  repos: { type: array, default: [alpha, beta] }
steps:
  - id: only
    agent: worker
    body: |
      scan since {{arg.window}} over {{arg.repos}}
---

prelude text
"#;
}
