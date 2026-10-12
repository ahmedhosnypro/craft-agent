//! Run and resume entry points; the replay fold lives in `resume`.

use std::collections::BTreeMap;

use crate::engine::{self, StepExecutor};
use crate::error::WorkflowError;
use crate::hash::sha256_hex;
use crate::journal::{Journal, JournalEvent};
use crate::plan::{self, ArgValue, WorkflowPlan};
use crate::resume::{Folded, fold};
use crate::state::EngineState;
use crate::status::WorkflowReport;

pub fn args_digest(args: &BTreeMap<String, ArgValue>) -> Result<String, WorkflowError> {
    let canonical = serde_json::to_string(args).map_err(|err| WorkflowError::Internal {
        detail: format!("args serialization failed: {err}"),
    })?;
    Ok(sha256_hex(&[canonical.as_bytes()]))
}

/// Deterministic run id derived from the plan and args digests.
pub fn run_id_for(plan_hash: &str, args_digest: &str) -> String {
    let head = &plan_hash[..16.min(plan_hash.len())];
    let tail = &args_digest[..8.min(args_digest.len())];
    format!("{head}-{tail}")
}

pub async fn run_workflow<E: StepExecutor + Sync>(
    plan: &WorkflowPlan,
    provided: &BTreeMap<String, ArgValue>,
    executor: &E,
    journal: &mut Journal,
) -> Result<WorkflowReport, WorkflowError> {
    if !journal.is_empty() {
        return Err(WorkflowError::JournalNotEmpty {
            path: journal.path_str(),
            entries: journal.len(),
        });
    }
    let args = plan::resolve_args(plan, provided)?;
    let digest = args_digest(&args)?;
    let run_id = run_id_for(&plan.plan_hash, &digest);
    journal.record(JournalEvent::RunStarted {
        seq: journal.next_seq(),
        run_id: run_id.clone(),
        name: plan.name.clone(),
        plan_hash: plan.plan_hash.clone(),
        args_digest: digest,
        limits: plan.limits,
    })?;
    let state = EngineState::new(plan);
    engine::drive(plan, &args, &run_id, executor, journal, state).await
}

pub async fn resume_workflow<E: StepExecutor + Sync>(
    plan: &WorkflowPlan,
    provided: &BTreeMap<String, ArgValue>,
    executor: &E,
    journal: &mut Journal,
) -> Result<WorkflowReport, WorkflowError> {
    if journal.is_empty() {
        return Err(WorkflowError::JournalEmpty {
            path: journal.path_str(),
        });
    }
    let args = plan::resolve_args(plan, provided)?;
    let digest = args_digest(&args)?;
    let (run_id, folded) = fold(plan, &args, &digest, journal)?;
    match folded {
        Folded::Finished(report) => Ok(report),
        Folded::Continue(state) => {
            engine::drive(plan, &args, &run_id, executor, journal, state).await
        }
    }
}
