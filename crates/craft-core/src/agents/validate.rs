//! Field rules: a parsed file becomes an [`AgentFile`] or one typed
//! error, mirroring the agent-file gate's rule table (required fields,
//! name charset and stem coherence, description shape and trigger
//! word, model shape, tool ids, and the body bounds).

use std::path::Path;

use super::parse::Parsed;
use super::{AgentFile, AgentFileError, PromptMode, TOOL_IDS, file_stem};

const DESC_BYTE_CAP: usize = 1024;
const BODY_LINE_LIMIT: usize = 300;
const PLACEHOLDERS: [&str; 3] = ["todo", "tbd", "wip"];
const TRIGGER_WORDS: [&str; 5] = ["use", "when", "run", "for", "against"];

fn entry<'a>(parsed: &'a Parsed, key: &str) -> Option<&'a super::parse::Entry> {
    parsed.entries.iter().find(|entry| entry.key == key)
}

/// Validates in the gate's rule order; the first violation wins.
pub(crate) fn build(parsed: &Parsed, path: &Path) -> Result<AgentFile, AgentFileError> {
    let name = required(parsed, path, "name")?;
    let description = required(parsed, path, "description")?;
    check_name(path, parsed, &name)?;
    check_description(path, parsed, &description)?;
    check_trigger(path, parsed, &description)?;
    let model = check_model(path, parsed)?;
    let tools = check_tools(path, parsed, "tools")?;
    let disallowed_tools = check_tools(path, parsed, "disallowedTools")?.unwrap_or_default();
    let prompt_mode = prompt_mode(parsed, path)?;
    let max_turns = number_field(parsed, "maxTurns", path)?;
    let timeout_secs = number_field(parsed, "timeoutSecs", path)?;
    let background = bool_field(parsed, "background", path, false)?;
    let agents_md = bool_field(parsed, "agentsMd", path, true)?;
    let body = check_body(parsed, path)?;
    Ok(AgentFile {
        path: path.to_path_buf(),
        name,
        description,
        model,
        prompt_mode,
        tools,
        disallowed_tools,
        max_turns,
        timeout_secs,
        background,
        agents_md,
        body,
    })
}

/// A required, non-empty scalar.
fn required(parsed: &Parsed, path: &Path, key: &str) -> Result<String, AgentFileError> {
    match entry(parsed, key) {
        None => Err(AgentFileError::RequiredField {
            path: path.to_path_buf(),
            line: 1,
            message: format!("missing required field: {key}"),
        }),
        Some(found) if found.value.is_empty() => Err(AgentFileError::RequiredField {
            path: path.to_path_buf(),
            line: found.line,
            message: format!("{key} is empty"),
        }),
        Some(found) => Ok(found.value.clone()),
    }
}

/// FM03 + FM04: a lowercase slug that names the file it lives in.
fn check_name(path: &Path, parsed: &Parsed, name: &str) -> Result<(), AgentFileError> {
    let line = entry(parsed, "name").map_or(1, |found| found.line);
    let charset_ok = name.len() <= 64
        && !name.contains("--")
        && !name.ends_with('-')
        && name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
        && name
            .chars()
            .skip(1)
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !charset_ok {
        return Err(AgentFileError::NameCharset {
            path: path.to_path_buf(),
            line,
            message: String::from(
                "name must be a lowercase slug (a-z, 0-9, single inner hyphens, \
                 at most 64 chars, no colon)",
            ),
        });
    }
    let stem = file_stem(path);
    if name != stem {
        return Err(AgentFileError::NameStem {
            path: path.to_path_buf(),
            line,
            message: format!("name '{name}' must equal the file stem '{stem}'"),
        });
    }
    Ok(())
}

/// FM05: non-placeholder, byte-bounded.
fn check_description(
    path: &Path,
    parsed: &Parsed,
    description: &str,
) -> Result<(), AgentFileError> {
    let line = entry(parsed, "description").map_or(1, |found| found.line);
    if description.len() > DESC_BYTE_CAP {
        return Err(AgentFileError::Description {
            path: path.to_path_buf(),
            line,
            message: format!(
                "description is {} bytes (cap {DESC_BYTE_CAP})",
                description.len()
            ),
        });
    }
    let lower = description.to_lowercase();
    if PLACEHOLDERS.contains(&lower.as_str()) {
        return Err(AgentFileError::Description {
            path: path.to_path_buf(),
            line,
            message: String::from("placeholder description"),
        });
    }
    Ok(())
}

/// FM06: the description tells the caller when to use the agent.
fn check_trigger(path: &Path, parsed: &Parsed, description: &str) -> Result<(), AgentFileError> {
    let lower = description.to_lowercase();
    if TRIGGER_WORDS.iter().any(|word| lower.contains(word)) {
        return Ok(());
    }
    Err(AgentFileError::Trigger {
        path: path.to_path_buf(),
        line: entry(parsed, "description").map_or(1, |found| found.line),
        message: String::from(
            "description must say when to use the agent (use/when/run/for/against)",
        ),
    })
}

/// MODEL01: `inherit` or a bare lowercase id.
fn check_model(path: &Path, parsed: &Parsed) -> Result<Option<String>, AgentFileError> {
    let Some(found) = entry(parsed, "model") else {
        return Ok(None);
    };
    let model = found.value.trim();
    if model == "inherit" {
        return Ok(None);
    }
    let mut chars = model.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let rest_ok = chars.all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '/' | '-')
    });
    if !first_ok || !rest_ok || model.is_empty() {
        return Err(AgentFileError::Model {
            path: path.to_path_buf(),
            line: found.line,
            message: String::from("model must be inherit or a bare lowercase id"),
        });
    }
    Ok(Some(model.to_string()))
}

/// TOOLS01: a comma or space separated list over the builtin ids;
/// brackets must balance and no id may be empty.
fn check_tools(
    path: &Path,
    parsed: &Parsed,
    field: &str,
) -> Result<Option<Vec<String>>, AgentFileError> {
    let Some(found) = entry(parsed, field) else {
        return Ok(None);
    };
    let raw = &found.value;
    let err = |message: String| AgentFileError::Tools {
        path: path.to_path_buf(),
        line: found.line,
        message,
    };
    let inner = if raw.starts_with('[') && raw.ends_with(']') && raw.len() >= 2 {
        raw.get(1..raw.len() - 1).unwrap_or("")
    } else if raw.starts_with('[') || raw.ends_with(']') {
        return Err(err(format!("unbalanced bracket list in {field}")));
    } else {
        raw.as_str()
    };
    if empty_tool_id(inner) {
        return Err(err(format!("empty tool id in {field}")));
    }
    let tokens: Vec<String> = inner
        .replace(',', " ")
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if tokens.is_empty() {
        return Err(err(format!("empty tool list in {field}")));
    }
    for token in &tokens {
        if !TOOL_IDS.contains(&token.as_str()) {
            return Err(err(format!("unknown tool id '{token}' in {field}")));
        }
    }
    Ok(Some(tokens))
}

/// Leading, trailing, or doubled commas (with any spaces between).
fn empty_tool_id(inner: &str) -> bool {
    if inner.trim_start_matches(' ').starts_with(',') || inner.trim_end_matches(' ').ends_with(',')
    {
        return true;
    }
    let mut after_comma = false;
    for char in inner.chars() {
        if char == ',' {
            if after_comma {
                return true;
            }
            after_comma = true;
        } else if char != ' ' {
            after_comma = false;
        }
    }
    false
}

/// FIELD01: whole numbers of at least one.
fn number_field<T>(parsed: &Parsed, key: &str, path: &Path) -> Result<Option<T>, AgentFileError>
where
    T: std::str::FromStr + PartialOrd + Default,
{
    let Some(found) = entry(parsed, key) else {
        return Ok(None);
    };
    let invalid = || AgentFileError::ValueShape {
        path: path.to_path_buf(),
        line: found.line,
        message: format!("{key} must be a whole number of at least 1"),
    };
    match found.value.trim().parse::<T>() {
        Ok(value) if value > T::default() => Ok(Some(value)),
        _ => Err(invalid()),
    }
}

/// FIELD01: `extend` or `full`; absent means extend.
fn prompt_mode(parsed: &Parsed, path: &Path) -> Result<PromptMode, AgentFileError> {
    let Some(found) = entry(parsed, "promptMode") else {
        return Ok(PromptMode::default());
    };
    match found.value.trim() {
        "extend" => Ok(PromptMode::Extend),
        "full" => Ok(PromptMode::Full),
        _ => Err(AgentFileError::ValueShape {
            path: path.to_path_buf(),
            line: found.line,
            message: String::from("promptMode must be extend or full"),
        }),
    }
}

/// FIELD01: literal true or false, `absent` when the key is missing.
fn bool_field(
    parsed: &Parsed,
    key: &str,
    path: &Path,
    absent: bool,
) -> Result<bool, AgentFileError> {
    let Some(found) = entry(parsed, key) else {
        return Ok(absent);
    };
    match found.value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AgentFileError::ValueShape {
            path: path.to_path_buf(),
            line: found.line,
            message: format!("{key} must be true or false"),
        }),
    }
}

/// MD01 + MD02: a present body within the effective-line cap; the
/// stored body is edge-trimmed.
fn check_body(parsed: &Parsed, path: &Path) -> Result<String, AgentFileError> {
    let body = parsed.body.trim();
    if body.is_empty() {
        return Err(AgentFileError::Body {
            path: path.to_path_buf(),
            line: parsed.close_line,
            message: String::from("no prompt body after the frontmatter"),
        });
    }
    let lines = effective_body_lines(&parsed.body);
    if lines > BODY_LINE_LIMIT {
        return Err(AgentFileError::BodyLength {
            path: path.to_path_buf(),
            line: parsed.close_line,
            message: format!("{} effective body lines (cap {BODY_LINE_LIMIT})", lines),
        });
    }
    Ok(body.to_string())
}

/// The gate's line accounting: blanks, `//` lines, and `/* */` blocks
/// do not count.
fn effective_body_lines(body: &str) -> usize {
    let mut in_block = false;
    let mut count = 0usize;
    for raw in body.lines() {
        let line = raw.trim_matches([' ', '\t', '\r']);
        if in_block {
            if line.contains("*/") {
                in_block = false;
            }
        } else if line.is_empty() || line.starts_with("//") {
            continue;
        } else if let Some(rest) = line.strip_prefix("/*") {
            if !rest.contains("*/") {
                in_block = true;
            }
        } else {
            count += 1;
        }
    }
    count
}
