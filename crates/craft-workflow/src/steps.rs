//! Step conversion: frontmatter step entries into validated StepSpecs.

use std::collections::BTreeSet;

use crate::error::WorkflowError;
use crate::plan::{
    DEFAULT_STEP_MAX_TURNS, MAX_RETRIES_CEILING, MAX_STEP_TURNS_CEILING, OnFailure, STEP_HARD_CAP,
    StepSpec, valid_key,
};
use crate::validate::parse_u64;
use crate::yaml::YValue;

const MAX_BODY_CHARS: usize = 16_384;
const MAX_NEEDS_PER_STEP: usize = 16;
const MAX_OUTPUTS_PER_STEP: usize = 16;

fn string_list(
    file: &str,
    step: &str,
    field: &str,
    value: Option<&YValue>,
    cap: usize,
) -> Result<Vec<String>, WorkflowError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value.as_seq().ok_or_else(|| WorkflowError::Parse {
        file: file.to_string(),
        detail: format!("step `{step}`: `{field}` must be a list"),
    })?;
    if items.len() > cap {
        return Err(WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step `{step}`: `{field}` exceeds {cap} entries"),
        });
    }
    let mut out = Vec::new();
    for item in items {
        let text = item.as_str().ok_or_else(|| WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step `{step}`: `{field}` entries must be strings"),
        })?;
        if !valid_key(text) {
            return Err(WorkflowError::Parse {
                file: file.to_string(),
                detail: format!("step `{step}`: `{field}` entry `{text}` is not a valid name"),
            });
        }
        out.push(text.to_string());
    }
    Ok(out)
}

fn convert_step(
    file: &str,
    index: usize,
    fields: &[(String, YValue)],
) -> Result<StepSpec, WorkflowError> {
    for (key, _) in fields {
        if !matches!(
            key.as_str(),
            "id" | "agent"
                | "model"
                | "needs"
                | "max-turns"
                | "retry"
                | "on-failure"
                | "outputs"
                | "body"
        ) {
            return Err(WorkflowError::UnknownField {
                file: file.to_string(),
                context: format!("step {index}"),
                field: key.clone(),
            });
        }
    }
    let id = match fields.iter().find(|(k, _)| k == "id") {
        Some((_, value)) => value.as_str().ok_or_else(|| WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step {index}: `id` must be a string"),
        })?,
        None => {
            return Err(WorkflowError::MissingStepId {
                file: file.to_string(),
                index,
            });
        }
    };
    if !valid_key(id) {
        return Err(WorkflowError::BadId {
            file: file.to_string(),
            raw: id.to_string(),
        });
    }
    let agent = match fields.iter().find(|(k, _)| k == "agent") {
        Some((_, value)) => value.as_str().ok_or_else(|| WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step `{id}`: `agent` must be a string"),
        })?,
        None => "",
    };
    if agent.is_empty() {
        return Err(WorkflowError::EmptyAgent {
            file: file.to_string(),
            step: id.to_string(),
        });
    }
    if agent.len() > 64 {
        return Err(WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step `{id}`: `agent` exceeds 64 chars"),
        });
    }
    let model = match fields.iter().find(|(k, _)| k == "model") {
        Some((_, value)) => {
            let text = value.as_str().ok_or_else(|| WorkflowError::Parse {
                file: file.to_string(),
                detail: format!("step `{id}`: `model` must be a string"),
            })?;
            if text.is_empty() || text.len() > 64 {
                return Err(WorkflowError::Parse {
                    file: file.to_string(),
                    detail: format!("step `{id}`: `model` must be 1..=64 chars"),
                });
            }
            Some(text.to_string())
        }
        None => None,
    };
    step_rest(file, id, agent, model, fields)
}

fn step_rest(
    file: &str,
    id: &str,
    agent: &str,
    model: Option<String>,
    fields: &[(String, YValue)],
) -> Result<StepSpec, WorkflowError> {
    let field = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| v);
    let needs = string_list(file, id, "needs", field("needs"), MAX_NEEDS_PER_STEP)?;
    let mut seen = BTreeSet::new();
    for need in &needs {
        if !seen.insert(need.clone()) {
            return Err(WorkflowError::DuplicateNeed {
                file: file.to_string(),
                step: id.to_string(),
                need: need.clone(),
            });
        }
    }
    let max_turns = match field("max-turns") {
        Some(value) => {
            let text = value.as_str().ok_or_else(|| WorkflowError::BadMaxTurns {
                file: file.to_string(),
                step: id.to_string(),
                detail: "must be a positive integer".to_string(),
            })?;
            let Some(number) = parse_u64(text) else {
                return Err(WorkflowError::BadMaxTurns {
                    file: file.to_string(),
                    step: id.to_string(),
                    detail: format!("`{text}` is not a positive integer"),
                });
            };
            if number == 0 {
                return Err(WorkflowError::BadMaxTurns {
                    file: file.to_string(),
                    step: id.to_string(),
                    detail: "must be positive".to_string(),
                });
            }
            if number > u64::from(MAX_STEP_TURNS_CEILING) {
                return Err(WorkflowError::BadMaxTurns {
                    file: file.to_string(),
                    step: id.to_string(),
                    detail: format!("`{number}` exceeds the {MAX_STEP_TURNS_CEILING} ceiling"),
                });
            }
            number as u32
        }
        None => DEFAULT_STEP_MAX_TURNS,
    };
    let retry = match field("retry") {
        Some(value) => {
            let text = value.as_str().ok_or_else(|| WorkflowError::BadRetry {
                file: file.to_string(),
                step: id.to_string(),
                value: "(non-string)".to_string(),
                cap: MAX_RETRIES_CEILING,
            })?;
            let Some(number) = parse_u64(text) else {
                return Err(WorkflowError::BadRetry {
                    file: file.to_string(),
                    step: id.to_string(),
                    value: text.to_string(),
                    cap: MAX_RETRIES_CEILING,
                });
            };
            if number > u64::from(MAX_RETRIES_CEILING) {
                return Err(WorkflowError::BadRetry {
                    file: file.to_string(),
                    step: id.to_string(),
                    value: text.to_string(),
                    cap: MAX_RETRIES_CEILING,
                });
            }
            number as u32
        }
        None => 0,
    };
    let on_failure = match field("on-failure") {
        Some(value) => {
            let text = value.as_str().ok_or_else(|| WorkflowError::BadOnFailure {
                file: file.to_string(),
                step: id.to_string(),
                value: "(non-string)".to_string(),
            })?;
            match text {
                "stop" => OnFailure::Stop,
                "skip" => OnFailure::Skip,
                _ => {
                    return Err(WorkflowError::BadOnFailure {
                        file: file.to_string(),
                        step: id.to_string(),
                        value: text.to_string(),
                    });
                }
            }
        }
        None => OnFailure::Stop,
    };
    let outputs = string_list(file, id, "outputs", field("outputs"), MAX_OUTPUTS_PER_STEP)?;
    let body = match field("body") {
        Some(value) => value.as_str().ok_or_else(|| WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step `{id}`: `body` must be a text block"),
        })?,
        None => {
            return Err(WorkflowError::MissingBody {
                file: file.to_string(),
                step: id.to_string(),
            });
        }
    };
    if body.chars().count() > MAX_BODY_CHARS {
        return Err(WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step `{id}`: `body` exceeds {MAX_BODY_CHARS} chars"),
        });
    }
    Ok(StepSpec {
        id: id.to_string(),
        agent: agent.to_string(),
        model,
        needs,
        max_turns,
        retry,
        on_failure,
        outputs,
        body: body.to_string(),
        dependants: Vec::new(),
    })
}

pub fn convert_steps(file: &str, value: Option<&YValue>) -> Result<Vec<StepSpec>, WorkflowError> {
    let Some(value) = value else {
        return Err(WorkflowError::MissingSteps {
            file: file.to_string(),
        });
    };
    let items = value.as_seq().ok_or_else(|| WorkflowError::Parse {
        file: file.to_string(),
        detail: "`steps` must be a list".to_string(),
    })?;
    if items.is_empty() {
        return Err(WorkflowError::MissingSteps {
            file: file.to_string(),
        });
    }
    if items.len() > STEP_HARD_CAP {
        return Err(WorkflowError::StepCountCap {
            file: file.to_string(),
            count: items.len(),
            cap: STEP_HARD_CAP,
        });
    }
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let fields = item.as_map().ok_or_else(|| WorkflowError::Parse {
            file: file.to_string(),
            detail: format!("step {index} must be a map"),
        })?;
        let spec = convert_step(file, index, fields)?;
        if out.iter().any(|other: &StepSpec| other.id == spec.id) {
            return Err(WorkflowError::DuplicateStepId {
                file: file.to_string(),
                id: spec.id,
            });
        }
        out.push(spec);
    }
    Ok(out)
}
