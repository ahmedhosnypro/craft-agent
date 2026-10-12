//! The frontmatter reader: a flat YAML-subset line machine producing
//! one joined value per key, mirroring the agent-file gate's parse
//! (folded scalars, `- ` list items, comma strings; blank, tabbed,
//! unparseable, duplicate, and unknown lines are errors).

use std::path::Path;

use super::{AgentFileError, KNOWN_KEYS};

/// One key with its joined value and 1-based source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub key: String,
    pub value: String,
    pub line: u32,
}

/// The split of one agent file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub entries: Vec<Entry>,
    /// 1-based line of the closing fence.
    pub close_line: u32,
    /// Raw body lines after the fence, joined with newlines.
    pub body: String,
}

fn fm_err(path: &Path, line: u32, message: impl Into<String>) -> AgentFileError {
    AgentFileError::Frontmatter {
        path: path.to_path_buf(),
        line,
        message: message.into(),
    }
}

/// Splits a file into frontmatter entries and the body. The first line
/// must be a `---` fence; the next bare `---` line closes it.
pub(crate) fn parse(source: &str, path: &Path) -> Result<Parsed, AgentFileError> {
    let lines: Vec<&str> = source.split('\n').map(strip_cr).collect();
    if lines.first().copied() != Some("---") {
        return Err(fm_err(path, 1, "file must start with a --- line"));
    }
    let Some(close) = (1..lines.len()).find(|&index| lines[index] == "---") else {
        return Err(fm_err(path, 1, "no closing --- line"));
    };
    if close == 1 {
        return Err(fm_err(path, close as u32 + 1, "frontmatter block is empty"));
    }
    let entries = parse_entries(&lines[1..close], path)?;
    Ok(Parsed {
        entries,
        close_line: close as u32 + 1,
        body: lines[close + 1..].join("\n"),
    })
}

fn strip_cr(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

/// A key line: `[A-Za-z][A-Za-z0-9_]*:` at column zero, returning the
/// key and the raw value after the colon.
fn key_line(raw: &str) -> Option<(&str, &str)> {
    let colon = raw.find(':')?;
    let key = raw.get(..colon)?;
    let first = key.chars().next()?;
    let rest_ok = key
        .chars()
        .skip(1)
        .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if first.is_ascii_alphabetic() && rest_ok {
        Some((key, raw.get(colon + 1..).unwrap_or("")))
    } else {
        None
    }
}

const BLOCK_INDICATORS: [&str; 6] = [">", ">-", ">+", "|", "|-", "|+"];

/// Walks the frontmatter lines, first-error wins in line order.
fn parse_entries(frontmatter: &[&str], path: &Path) -> Result<Vec<Entry>, AgentFileError> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut key = String::new();
    let mut key_line_no = 0u32;
    let mut parts: Vec<String> = Vec::new();

    for (index, raw) in frontmatter.iter().enumerate() {
        let line = index as u32 + 2;
        if raw.contains('\t') {
            return Err(fm_err(path, line, "tab character in frontmatter"));
        }
        if let Some((found, value)) = key_line(raw) {
            flush(&mut entries, &key, key_line_no, &mut parts);
            if seen.iter().any(|k| k == found) {
                return Err(fm_err(path, line, format!("duplicate key: {found}")));
            }
            if !KNOWN_KEYS.contains(&found) {
                return Err(fm_err(
                    path,
                    line,
                    format!("unknown frontmatter key: {found}"),
                ));
            }
            seen.push(found.to_string());
            key = found.to_string();
            key_line_no = line;
            parts = Vec::new();
            let value = value.trim_start_matches([' ', '\t']);
            if !BLOCK_INDICATORS.contains(&value) {
                parts.push(value.to_string());
            }
        } else if raw.is_empty() || raw.bytes().all(|byte| byte == b' ') {
            return Err(fm_err(path, line, "blank line in frontmatter"));
        } else if let Some(content) = continuation(raw) {
            if key.is_empty() {
                return Err(fm_err(path, line, "list or continuation line with no key"));
            }
            parts.push(content.to_string());
        } else {
            return Err(fm_err(path, line, "unparseable frontmatter line"));
        }
    }
    flush(&mut entries, &key, key_line_no, &mut parts);
    Ok(entries)
}

/// A continuation line: `- item` or any indented line, leading spaces
/// and one `- ` prefix stripped.
fn continuation(raw: &str) -> Option<&str> {
    if let Some(content) = raw.strip_prefix("- ") {
        return Some(content);
    }
    let stripped = raw.strip_prefix(' ')?.trim_start_matches(' ');
    Some(stripped.strip_prefix("- ").unwrap_or(stripped))
}

/// Emits the pending key with its parts trimmed and joined by single
/// spaces, mirroring the gate's value folding.
fn flush(entries: &mut Vec<Entry>, key: &str, line: u32, parts: &mut Vec<String>) {
    if key.is_empty() {
        return;
    }
    let mut value = String::new();
    for part in parts.drain(..) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if value.is_empty() {
            value.push_str(part);
        } else {
            value.push(' ');
            value.push_str(part);
        }
    }
    entries.push(Entry {
        key: key.to_string(),
        value,
        line,
    });
}

/// True when the frontmatter's structural rules would reject the
/// content before any field rule runs.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_line_recovers_key_and_value() {
        assert_eq!(key_line("name: repo-scout"), Some(("name", " repo-scout")));
        assert_eq!(key_line("tools:read_file"), Some(("tools", "read_file")));
        assert_eq!(key_line("a-b: x"), None);
        assert_eq!(key_line("no colon"), None);
    }
}
