//! Arg specs: frontmatter arg parsing and runtime arg resolution.

use std::collections::BTreeMap;

use crate::error::WorkflowError;
use crate::plan::{ArgKind, ArgSpec, ArgValue, MAX_ARGS, PREVIEW_CHARS, WorkflowPlan, valid_key};
use crate::truncate_chars;

pub(crate) fn parse_args(
    file: &str,
    value: Option<&crate::yaml::YValue>,
) -> Result<BTreeMap<String, ArgSpec>, WorkflowError> {
    let mut out = BTreeMap::new();
    let Some(value) = value else {
        return Ok(out);
    };
    let entries = value.as_map().ok_or_else(|| WorkflowError::Parse {
        file: file.to_string(),
        detail: "`args` must be a map".to_string(),
    })?;
    if entries.len() > MAX_ARGS {
        return Err(WorkflowError::BadArgSpec {
            file: file.to_string(),
            name: "(all)".to_string(),
            detail: format!("more than {MAX_ARGS} args"),
        });
    }
    for (name, spec) in entries {
        if !valid_key(name) {
            return Err(WorkflowError::BadArgSpec {
                file: file.to_string(),
                name: name.clone(),
                detail: "invalid arg name".to_string(),
            });
        }
        let fields = spec.as_map().ok_or_else(|| WorkflowError::BadArgSpec {
            file: file.to_string(),
            name: name.clone(),
            detail: "must be a map with a `type`".to_string(),
        })?;
        for (key, _) in fields {
            if !matches!(key.as_str(), "type" | "default") {
                return Err(WorkflowError::UnknownField {
                    file: file.to_string(),
                    context: format!("arg `{name}`"),
                    field: key.clone(),
                });
            }
        }
        let kind = match spec
            .get("type")
            .and_then(|v| v.as_str())
            .and_then(ArgKind::parse)
        {
            Some(kind) => kind,
            None => {
                return Err(WorkflowError::BadArgSpec {
                    file: file.to_string(),
                    name: name.clone(),
                    detail: "`type` must be string|array".to_string(),
                });
            }
        };
        let default = match spec.get("default") {
            None => None,
            Some(value) => Some(match (kind, value) {
                (ArgKind::String, crate::yaml::YValue::Str(text)) => {
                    ArgValue::Str(truncate_chars(text, PREVIEW_CHARS))
                }
                (ArgKind::Array, crate::yaml::YValue::Seq(items)) => {
                    let mut list = Vec::new();
                    for item in items {
                        let text = item.as_str().ok_or_else(|| WorkflowError::BadArgSpec {
                            file: file.to_string(),
                            name: name.clone(),
                            detail: "array default items must be strings".to_string(),
                        })?;
                        list.push(truncate_chars(text, PREVIEW_CHARS));
                    }
                    ArgValue::Seq(list)
                }
                _ => {
                    return Err(WorkflowError::BadArgSpec {
                        file: file.to_string(),
                        name: name.clone(),
                        detail: "default kind does not match `type`".to_string(),
                    });
                }
            }),
        };
        out.insert(name.clone(), ArgSpec { kind, default });
    }
    Ok(out)
}

pub fn resolve_args(
    plan: &WorkflowPlan,
    provided: &BTreeMap<String, ArgValue>,
) -> Result<BTreeMap<String, ArgValue>, WorkflowError> {
    for name in provided.keys() {
        if !plan.args.contains_key(name) {
            return Err(WorkflowError::ArgNotDeclared { name: name.clone() });
        }
    }
    let mut out = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();
    for (name, spec) in &plan.args {
        match provided.get(name) {
            Some(value) => {
                let kind_ok = matches!(
                    (spec.kind, value),
                    (ArgKind::String, ArgValue::Str(_)) | (ArgKind::Array, ArgValue::Seq(_))
                );
                if !kind_ok {
                    let expected = match spec.kind {
                        ArgKind::String => "a string",
                        ArgKind::Array => "an array",
                    };
                    return Err(WorkflowError::BadArgValue {
                        name: name.clone(),
                        detail: format!("expected {expected}"),
                    });
                }
                out.insert(name.clone(), value.clone());
            }
            None => match &spec.default {
                Some(default) => {
                    out.insert(name.clone(), default.clone());
                }
                None => missing.push(name.clone()),
            },
        }
    }
    if !missing.is_empty() {
        return Err(WorkflowError::ArgsMissing {
            names: missing.join(", "),
        });
    }
    Ok(out)
}
