//! Terminal-row verification: a finished journal must replay to the same
//! conclusion the folded step table implies, or the resume diverges.

use crate::error::WorkflowError;
use crate::plan::WorkflowPlan;
use crate::resume::Folded;
use crate::state::{EngineState, Slot};
use crate::status::{RunConclusion, SkipReason, StepReport, StepStatus, StepUsage, WorkflowReport};

#[allow(clippy::too_many_arguments)]
pub(crate) fn verify_finished(
    plan: &WorkflowPlan,
    state: &EngineState,
    path: &str,
    run_id: &str,
    conclusion: &RunConclusion,
    failed_step: &Option<String>,
    total_usage: &StepUsage,
) -> Result<(String, Folded), WorkflowError> {
    for spec in &plan.steps {
        if !matches!(state.slots.get(&spec.id), Some(Slot::Done(_))) {
            return Err(WorkflowError::JournalParse {
                path: path.to_string(),
                line: 0,
                detail: format!("run_finished before step `{}` settled", spec.id),
            });
        }
    }
    let has_skip = state
        .slots
        .values()
        .any(|slot| matches!(slot, Slot::Done(StepStatus::Skipped { .. })));
    let recomputed = match &state.first_failure {
        Some((step, failed)) => RunConclusion::Failed {
            step: step.clone(),
            conclusion: *failed,
        },
        None if has_skip => RunConclusion::BudgetExhausted,
        None => RunConclusion::Completed,
    };
    if &recomputed != conclusion {
        return Err(WorkflowError::Divergence {
            detail: format!(
                "run_finished conclusion does not match the folded steps (journal `{conclusion:?}`, folded `{recomputed:?}`)"
            ),
        });
    }
    let expected_failed = state.first_failure.as_ref().map(|(step, _)| step.clone());
    if *failed_step != expected_failed {
        return Err(WorkflowError::Divergence {
            detail: "run_finished failed_step does not match the folded steps".to_string(),
        });
    }
    if total_usage != &state.total_usage {
        return Err(WorkflowError::Divergence {
            detail: "run_finished total_usage does not match the folded steps".to_string(),
        });
    }
    let steps = plan
        .steps
        .iter()
        .map(|spec| StepReport {
            id: spec.id.clone(),
            status: match state.slots.get(&spec.id) {
                Some(Slot::Done(status)) => status.clone(),
                _ => StepStatus::Skipped {
                    reason: SkipReason::Unreachable,
                },
            },
        })
        .collect();
    let report = WorkflowReport {
        run_id: run_id.to_string(),
        name: plan.name.clone(),
        conclusion: conclusion.clone(),
        exit_code: conclusion.exit_code(),
        failed_step: failed_step.clone(),
        steps,
        total_usage: *total_usage,
    };
    Ok((run_id.to_string(), Folded::Finished(report)))
}
