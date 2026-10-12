//! Crate error type: every failure is typed and actionable, never a panic.

use thiserror::Error;

/// Every failure mode of the workflow engine, closed set.
#[derive(Debug, Clone, Error)]
pub enum WorkflowError {
    #[error("workflow file `{file}`: {detail}")]
    Parse { file: String, detail: String },
    #[error("workflow file `{file}`: `name` is required")]
    MissingName { file: String },
    #[error("workflow file `{file}`: `steps` must list at least one step")]
    MissingSteps { file: String },
    #[error("workflow file `{file}`: {count} steps exceeds the {cap}-step plan cap")]
    StepCountCap {
        file: String,
        count: usize,
        cap: usize,
    },
    #[error("workflow file `{file}`: duplicate step id `{id}`")]
    DuplicateStepId { file: String, id: String },
    #[error("workflow file `{file}`: step `{step}` needs unknown step `{need}`")]
    UnknownNeed {
        file: String,
        step: String,
        need: String,
    },
    #[error("workflow file `{file}`: step `{step}` lists dependency `{need}` twice")]
    DuplicateNeed {
        file: String,
        step: String,
        need: String,
    },
    #[error("workflow file `{file}`: steps form a cycle: {path}")]
    Cycle { file: String, path: String },
    #[error("workflow file `{file}`: limit `{field}` {detail}")]
    BadLimit {
        file: String,
        field: String,
        detail: String,
    },
    #[error("workflow file `{file}`: step `{step}` has no agent")]
    EmptyAgent { file: String, step: String },
    #[error(
        "workflow file `{file}`: step `{step}` retry `{value}` is not a retry count (0..={cap})"
    )]
    BadRetry {
        file: String,
        step: String,
        value: String,
        cap: u32,
    },
    #[error("workflow file `{file}`: step `{step}` on-failure `{value}` is not stop|skip")]
    BadOnFailure {
        file: String,
        step: String,
        value: String,
    },
    #[error("workflow file `{file}`: step `{step}` max-turns {detail}")]
    BadMaxTurns {
        file: String,
        step: String,
        detail: String,
    },
    #[error(
        "workflow file `{file}`: invalid step id `{raw}` (1..=64 chars of a-z, 0-9, dot, dash, underscore)"
    )]
    BadId { file: String, raw: String },
    #[error("workflow file `{file}`: step {index} has no id")]
    MissingStepId { file: String, index: usize },
    #[error("workflow file `{file}`: step `{step}` has no body")]
    MissingBody { file: String, step: String },
    #[error(
        "workflow file `{file}`: step `{step}` declares `on-failure: skip` but `{dependent}` depends on it"
    )]
    SkipHasDependants {
        file: String,
        step: String,
        dependent: String,
    },
    #[error("workflow file `{file}`: unknown field `{field}` in {context}")]
    UnknownField {
        file: String,
        context: String,
        field: String,
    },
    #[error("workflow file `{file}`: step `{step}` references undeclared arg `{name}`")]
    UnknownArgRef {
        file: String,
        step: String,
        name: String,
    },
    #[error("workflow file `{file}`: arg `{name}` {detail}")]
    BadArgSpec {
        file: String,
        name: String,
        detail: String,
    },
    #[error("workflow args not provided: {names}")]
    ArgsMissing { names: String },
    #[error("workflow arg `{name}` is not declared by the plan")]
    ArgNotDeclared { name: String },
    #[error("workflow arg `{name}`: {detail}")]
    BadArgValue { name: String, detail: String },
    #[error("journal `{path}`: io failed: {detail}")]
    JournalIo { path: String, detail: String },
    #[error("journal `{path}`: line {line} is not a valid event: {detail}")]
    JournalParse {
        path: String,
        line: u64,
        detail: String,
    },
    #[error("journal `{path}`: entry {actual} is out of sequence (expected {expected})")]
    JournalSequence {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error("journal `{path}`: exceeds the {limit}-byte cap")]
    JournalCap { path: String, limit: u64 },
    #[error("journal `{path}`: refusing to grow past the {limit}-entry cap at entry {seq}")]
    JournalFull {
        path: String,
        seq: u64,
        limit: usize,
    },
    #[error("journal `{path}` is unsafe to open: {reason}")]
    JournalUnsafe { path: String, reason: String },
    #[error(
        "journal `{path}` already holds {entries} events; resume it instead of running over it"
    )]
    JournalNotEmpty { path: String, entries: usize },
    #[error("cannot resume: journal `{path}` is empty")]
    JournalEmpty { path: String },
    #[error("workflow diverged from the journal: {detail}")]
    Divergence { detail: String },
    #[error("step `{step}` could not run: {detail}")]
    StepLaunch { step: String, detail: String },
    #[error("scheduler exceeded its iteration cap ({cap})")]
    SchedulerCap { cap: usize },
    #[error("internal scheduler invariant broken: {detail}")]
    Internal { detail: String },
}
