//! Workflow engine: declarative frontmatter DAGs driving bounded step runs.
//!
//! Pure core with no provider or runtime dependency — the caller supplies a
//! [`StepExecutor`](engine::StepExecutor) and drives the engine per step.

pub mod args;
pub mod assemble;
pub mod engine;
pub mod error;
pub mod graph;
pub mod hash;
pub mod journal;
pub mod plan;
pub mod pool;
pub mod resume;
pub mod run;
pub mod state;
pub mod status;
pub mod steps;
pub mod validate;
pub mod verify;
pub mod yaml;
pub mod yaml_flow;

pub use assemble::{StepInputs, assemble_step};
pub use engine::{StepExecutor, StepOutcome};
pub use error::WorkflowError;
pub use journal::{Journal, JournalEvent};
pub use plan::{ArgKind, ArgSpec, ArgValue, Limits, OnFailure, StepSpec, WorkflowPlan};
pub use run::{resume_workflow, run_workflow};
pub use status::{
    RunConclusion, SkipReason, StepConclusion, StepReport, StepStatus, StepUsage, WorkflowReport,
};

/// Truncate to at most `cap` chars, never splitting a char.
pub fn truncate_chars(text: &str, cap: usize) -> String {
    match text.char_indices().nth(cap) {
        Some((idx, _)) => text[..idx].to_string(),
        None => text.to_string(),
    }
}
