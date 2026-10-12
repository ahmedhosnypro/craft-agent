//! The resume fold: deterministic journal replay with divergence checks.

use std::collections::BTreeMap;

use crate::assemble::{assemble_step, previews_for};
use crate::error::WorkflowError;
use crate::journal::{Journal, JournalEvent};
use crate::plan::{ArgValue, PREVIEW_CHARS, WorkflowPlan};
use crate::state::{EngineState, Slot};
use crate::status::{StepStatus, StepUsage, WorkflowReport};
use crate::truncate_chars;

pub(crate) enum Folded {
    Finished(WorkflowReport),
    Continue(EngineState),
}

use crate::verify::verify_finished;

pub(crate) fn fold(
    plan: &WorkflowPlan,
    args: &BTreeMap<String, ArgValue>,
    digest: &str,
    journal: &Journal,
) -> Result<(String, Folded), WorkflowError> {
    let path = journal.path_str();
    let entries = journal.entries();
    let Some(JournalEvent::RunStarted {
        run_id,
        plan_hash,
        args_digest,
        ..
    }) = entries.first()
    else {
        return Err(WorkflowError::JournalParse {
            path,
            line: 1,
            detail: "the first journal event is not run_started".to_string(),
        });
    };
    if *plan_hash != plan.plan_hash {
        return Err(WorkflowError::Divergence {
            detail: format!(
                "plan hash mismatch (journal `{plan_hash}`, plan `{}`)",
                plan.plan_hash
            ),
        });
    }
    if *args_digest != digest {
        return Err(WorkflowError::Divergence {
            detail: format!("args digest mismatch (journal `{args_digest}`, current `{digest}`)"),
        });
    }
    let mut state = EngineState::new(plan);
    for (i, event) in entries.iter().enumerate().skip(1) {
        let line = (i + 1) as u64;
        match event {
            JournalEvent::RunFinished {
                conclusion,
                failed_step,
                total_usage,
                ..
            } => {
                if i + 1 != entries.len() {
                    return Err(WorkflowError::JournalParse {
                        path,
                        line,
                        detail: "run_finished is not the last event".to_string(),
                    });
                }
                return verify_finished(
                    plan,
                    &state,
                    &path,
                    run_id,
                    conclusion,
                    failed_step,
                    total_usage,
                );
            }
            JournalEvent::RunStarted { .. } => {
                return Err(WorkflowError::JournalParse {
                    path,
                    line,
                    detail: "run_started appears twice".to_string(),
                });
            }
            JournalEvent::StepStarted { id, .. } => {
                fold_started(plan, &mut state, id, &path, line)?;
            }
            JournalEvent::StepFinished {
                id,
                input_hash,
                conclusion,
                usage,
                final_message,
                ..
            } => {
                fold_finished(
                    plan,
                    args,
                    &mut state,
                    id,
                    input_hash,
                    conclusion,
                    usage,
                    final_message,
                    &path,
                    line,
                )?;
            }
            JournalEvent::StepSkipped { id, reason, .. } => {
                fold_skipped(plan, &mut state, id, reason, &path, line)?;
            }
        }
    }
    requeue(plan, &mut state);
    Ok((run_id.clone(), Folded::Continue(state)))
}

fn fold_started(
    plan: &WorkflowPlan,
    state: &mut EngineState,
    id: &str,
    path: &str,
    line: u64,
) -> Result<(), WorkflowError> {
    let corrupt = |detail: String| WorkflowError::JournalParse {
        path: path.to_string(),
        line,
        detail,
    };
    if plan.step(id).is_none() {
        return Err(corrupt(format!("journal names unknown step `{id}`")));
    }
    if matches!(state.slots.get(id), Some(Slot::Done(_))) {
        return Err(corrupt(format!("step `{id}` started after it settled")));
    }
    if state.counted.insert(id.to_string()) {
        state.pool.start();
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn fold_finished(
    plan: &WorkflowPlan,
    args: &BTreeMap<String, ArgValue>,
    state: &mut EngineState,
    id: &str,
    input_hash: &str,
    conclusion: &crate::status::StepConclusion,
    usage: &StepUsage,
    final_message: &str,
    path: &str,
    line: u64,
) -> Result<(), WorkflowError> {
    let corrupt = |detail: String| WorkflowError::JournalParse {
        path: path.to_string(),
        line,
        detail,
    };
    let Some(spec) = plan.step(id) else {
        return Err(corrupt(format!("journal names unknown step `{id}`")));
    };
    if !state.counted.contains(id) {
        return Err(corrupt(format!("step `{id}` finished without a start")));
    }
    if matches!(state.slots.get(id), Some(Slot::Done(_))) {
        return Err(corrupt(format!("step `{id}` settles twice")));
    }
    for need in &spec.needs {
        if !state.is_settled_success(need) {
            return Err(corrupt(format!(
                "step `{id}` settled before its dependency `{need}`"
            )));
        }
    }
    let previews = previews_for(spec, |need| state.settled_preview(need));
    let inputs = assemble_step(plan, args, spec, &previews);
    if inputs.input_hash() != input_hash {
        return Err(WorkflowError::Divergence {
            detail: format!(
                "step `{id}` input hash mismatch (journal `{input_hash}`, recomputed `{}`)",
                inputs.input_hash()
            ),
        });
    }
    let preview = truncate_chars(final_message, PREVIEW_CHARS);
    state.total_usage = state.total_usage.saturating_add(*usage);
    state.pool.settle(*usage);
    if !conclusion.is_success() && state.first_failure.is_none() {
        state.first_failure = Some((id.to_string(), *conclusion));
    }
    state.slots.insert(
        id.to_string(),
        Slot::Done(StepStatus::Settled {
            conclusion: *conclusion,
            usage: *usage,
            final_message: preview,
        }),
    );
    Ok(())
}

fn fold_skipped(
    plan: &WorkflowPlan,
    state: &mut EngineState,
    id: &str,
    reason: &crate::status::SkipReason,
    path: &str,
    line: u64,
) -> Result<(), WorkflowError> {
    let corrupt = |detail: String| WorkflowError::JournalParse {
        path: path.to_string(),
        line,
        detail,
    };
    if plan.step(id).is_none() {
        return Err(corrupt(format!("journal names unknown step `{id}`")));
    }
    if matches!(state.slots.get(id), Some(Slot::Done(_))) {
        return Err(corrupt(format!("step `{id}` settles twice")));
    }
    if let crate::status::SkipReason::DependencyFailed { dep } = reason {
        let broken = match state.slots.get(dep) {
            Some(Slot::Done(StepStatus::Settled { conclusion, .. })) => !conclusion.is_success(),
            Some(Slot::Done(StepStatus::Skipped { .. })) => true,
            _ => false,
        };
        if !broken {
            return Err(corrupt(format!(
                "step `{id}` was skipped over healthy dependency `{dep}`"
            )));
        }
    }
    state.slots.insert(
        id.to_string(),
        Slot::Done(StepStatus::Skipped {
            reason: reason.clone(),
        }),
    );
    Ok(())
}

/// Pending steps are re-queued when every dependency settled successfully;
/// prior consumption stays charged against the pool.
fn requeue(plan: &WorkflowPlan, state: &mut EngineState) {
    state.ready.clear();
    for spec in &plan.steps {
        let unmet = spec
            .needs
            .iter()
            .filter(|need| !state.is_settled_success(need))
            .count() as u32;
        state.unmet.insert(spec.id.clone(), unmet);
        if matches!(state.slots.get(&spec.id), Some(Slot::Pending)) && unmet == 0 {
            state.ready.push_back(spec.id.clone());
        }
    }
}
