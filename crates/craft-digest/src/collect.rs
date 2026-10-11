//! Deterministic gh collectors over the suite repositories, run directly
//! via std::process::Command.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use crate::bundle::Bundle;
use crate::error::{DigestError, bounded};
use crate::sanitize::sanitize;
use crate::window::{Window, is_valid_date};

/// PR bodies are kept only up to this byte prefix, on a char boundary.
pub const PR_BODY_PREFIX_BYTES: usize = 2000;
/// Newest merged PRs recorded per repo.
pub const MAX_PRS_PER_REPO: usize = 30;
/// gh row limit; a full page may hide older PRs and is recorded as such.
pub const GH_ROW_LIMIT: usize = 201;
const MAX_LABELS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRecord {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub merged_at: String,
    pub additions: u64,
    pub deletions: u64,
    pub labels: Vec<String>,
    pub body: String,
}

pub struct GhRunner {
    program: PathBuf,
}

impl Default for GhRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl GhRunner {
    pub fn new() -> Self {
        GhRunner {
            program: PathBuf::from("gh"),
        }
    }

    /// Tests point this at a fake gh script.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        GhRunner {
            program: program.into(),
        }
    }
}

/// Curated list lines: bare repo names, blanks and `#` comments dropped.
pub fn parse_repo_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Truncate to a byte prefix without splitting a char.
fn truncate_prefix(text: &str, cap_bytes: usize) -> String {
    if text.len() <= cap_bytes {
        return text.to_string();
    }
    let mut end = cap_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Parse `gh pr list --json` rows; untrusted, capped at GH_ROW_LIMIT rows.
pub fn parse_pr_rows(output: &str) -> Result<Vec<PrRecord>, DigestError> {
    let value: Value = serde_json::from_str(output)
        .map_err(|err| DigestError::GhOutput(bounded(&err.to_string())))?;
    let rows = value
        .as_array()
        .ok_or_else(|| DigestError::GhOutput(bounded("expected a JSON array of PR rows")))?;
    let mut prs = Vec::new();
    for row in rows.iter().take(GH_ROW_LIMIT) {
        if let Some(record) = parse_row(row) {
            prs.push(record);
        }
    }
    Ok(prs)
}

/// Rows missing number/title/url are dropped; the rest fall back safely.
fn parse_row(row: &Value) -> Option<PrRecord> {
    let number = row.get("number")?.as_u64()?;
    let title = sanitize(row.get("title").and_then(Value::as_str)?);
    let url = sanitize(row.get("url").and_then(Value::as_str)?);
    let merged_at = row
        .get("mergedAt")
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_default();
    let author = row
        .get("author")
        .and_then(|author| author.get("login"))
        .and_then(Value::as_str)
        .map(sanitize)
        .unwrap_or_else(|| "unknown".to_string());
    let additions = row.get("additions").and_then(Value::as_u64).unwrap_or(0);
    let deletions = row.get("deletions").and_then(Value::as_u64).unwrap_or(0);
    let labels = label_names(row);
    let body = row
        .get("body")
        .and_then(Value::as_str)
        .map(|body| truncate_prefix(&sanitize(body), PR_BODY_PREFIX_BYTES))
        .unwrap_or_default();
    Some(PrRecord {
        number,
        title,
        url,
        author,
        merged_at,
        additions,
        deletions,
        labels,
        body,
    })
}

fn label_names(row: &Value) -> Vec<String> {
    let Some(labels) = row.get("labels").and_then(Value::as_array) else {
        return Vec::new();
    };
    labels
        .iter()
        .filter_map(|label| label.get("name").and_then(Value::as_str))
        .map(sanitize)
        .take(MAX_LABELS)
        .collect()
}

/// Collect every repo; a failing repo becomes an error row, never a fatal Err.
pub fn collect_all(runner: &GhRunner, org: &str, repos: &[String], window: &Window) -> Bundle {
    let mut bundle = Bundle::new(org.to_string(), window.clone());
    for repo in repos {
        collect_repo(runner, org, repo, window, &mut bundle);
    }
    bundle.enforce_budget();
    bundle
}

fn collect_repo(runner: &GhRunner, org: &str, repo: &str, window: &Window, bundle: &mut Bundle) {
    let output = Command::new(&runner.program)
        .arg("pr")
        .arg("list")
        .arg("-R")
        .arg(format!("{org}/{repo}"))
        .arg("--state")
        .arg("merged")
        .arg("--limit")
        .arg(GH_ROW_LIMIT.to_string())
        .arg("--json")
        .arg("number,title,url,author,mergedAt,additions,deletions,labels,body")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => {
            bundle.push_error(repo, "gh", &err.to_string());
            bundle.push_repo(repo.to_string(), Vec::new());
            return;
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bundle.push_error(repo, "gh", &stderr);
        bundle.push_repo(repo.to_string(), Vec::new());
        return;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let prs = match parse_pr_rows(&stdout) {
        Ok(prs) => prs,
        Err(err) => {
            bundle.push_error(repo, "gh-output", &err.to_string());
            bundle.push_repo(repo.to_string(), Vec::new());
            return;
        }
    };
    if prs.len() >= GH_ROW_LIMIT {
        bundle.push_error(repo, "pr-list", "row limit reached; newest kept");
    }
    let mut in_window: Vec<PrRecord> = prs
        .into_iter()
        .filter(|pr| merged_in_window(pr, window))
        .collect();
    in_window.sort_by(|a, b| b.merged_at.cmp(&a.merged_at));
    in_window.truncate(MAX_PRS_PER_REPO);
    bundle.push_repo(repo.to_string(), in_window);
}

/// `mergedAt` is RFC 3339 UTC; only its date prefix is compared, and it
/// must be a real date to count.
fn merged_in_window(pr: &PrRecord, window: &Window) -> bool {
    match pr.merged_at.get(..10) {
        Some(date) => {
            is_valid_date(date) && date >= window.since.as_str() && date <= window.until.as_str()
        }
        None => false,
    }
}
