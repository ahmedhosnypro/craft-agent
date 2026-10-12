//! Fail-closed limits validation — all before any execution or model spend.

use crate::error::WorkflowError;
use crate::plan::{
    Limits, MAX_CONCURRENT_CEILING, MAX_RETRIES_CEILING, MAX_STEPS_CEILING,
    MAX_TOTAL_TOKENS_CEILING, MAX_TOTAL_TURNS_CEILING,
};
use crate::yaml::YValue;

pub(crate) fn parse_u64(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    trimmed.parse::<u64>().ok()
}

fn positive_within(
    file: &str,
    field: &str,
    raw: &str,
    number: u64,
    ceiling: u64,
) -> Result<(), WorkflowError> {
    if number == 0 {
        return Err(WorkflowError::BadLimit {
            file: file.to_string(),
            field: field.to_string(),
            detail: "must be positive".to_string(),
        });
    }
    if number > ceiling {
        return Err(WorkflowError::BadLimit {
            file: file.to_string(),
            field: field.to_string(),
            detail: format!("value `{raw}` exceeds the {ceiling} ceiling"),
        });
    }
    Ok(())
}

pub fn convert_limits(file: &str, value: Option<&YValue>) -> Result<Limits, WorkflowError> {
    let mut limits = Limits::default();
    let Some(value) = value else {
        return Ok(limits);
    };
    let entries = value.as_map().ok_or_else(|| WorkflowError::Parse {
        file: file.to_string(),
        detail: "`limits` must be a map".to_string(),
    })?;
    for (field, raw) in entries {
        let Some(text) = raw.as_str() else {
            return Err(WorkflowError::BadLimit {
                file: file.to_string(),
                field: field.clone(),
                detail: "must be a positive integer".to_string(),
            });
        };
        let Some(number) = parse_u64(text) else {
            return Err(WorkflowError::BadLimit {
                file: file.to_string(),
                field: field.clone(),
                detail: format!("`{text}` is not a positive integer"),
            });
        };
        match field.as_str() {
            "max-steps" => {
                positive_within(file, field, text, number, u64::from(MAX_STEPS_CEILING))?;
                limits.max_steps = number as u32;
            }
            "max-total-turns" => {
                positive_within(file, field, text, number, MAX_TOTAL_TURNS_CEILING)?;
                limits.max_total_turns = number;
            }
            "max-total-tokens" => {
                positive_within(file, field, text, number, MAX_TOTAL_TOKENS_CEILING)?;
                limits.max_total_tokens = number;
            }
            "max-retries-per-step" => {
                positive_within(file, field, text, number, u64::from(MAX_RETRIES_CEILING))?;
                limits.max_retries_per_step = number as u32;
            }
            "max-concurrent-steps" => {
                positive_within(file, field, text, number, u64::from(MAX_CONCURRENT_CEILING))?;
                limits.max_concurrent_steps = number as u32;
            }
            _ => {
                return Err(WorkflowError::UnknownField {
                    file: file.to_string(),
                    context: "limits".to_string(),
                    field: field.clone(),
                });
            }
        }
    }
    Ok(limits)
}
