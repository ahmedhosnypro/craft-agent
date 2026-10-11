//! Pull-request row parsing over untrusted REST bodies.

use serde::Serialize;
use serde_json::Value;

use crate::error::{HelperError, bounded, truncate_chars};

/// Rows kept per page; also the cap on a hostile oversized page.
pub const MAX_PULL_ROWS: usize = 100;
/// File rows kept per pull request (the first REST page).
pub const MAX_FILE_ROWS: usize = 100;
/// Any one string field is capped on a char boundary.
const FIELD_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRow {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub merged_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRow {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PullDetail {
    pub additions: u64,
    pub deletions: u64,
}

fn parse_json(body: &str) -> Result<Value, HelperError> {
    serde_json::from_str(body).map_err(|err| HelperError::MalformedJson(bounded(&err.to_string())))
}

fn field(value: &Value, name: &str) -> String {
    truncate_chars(
        value.get(name).and_then(Value::as_str).unwrap_or_default(),
        FIELD_CHARS,
    )
}

/// A pulls-list page; rows missing number/title/url are dropped.
pub fn parse_pull_page(body: &str) -> Result<Vec<PullRow>, HelperError> {
    let value = parse_json(body)?;
    let rows = value
        .as_array()
        .ok_or_else(|| HelperError::MalformedJson("expected a JSON array of pull rows".into()))?;
    let mut pulls = Vec::new();
    for row in rows.iter().take(MAX_PULL_ROWS) {
        let number = row.get("number").and_then(Value::as_u64);
        let title = row.get("title").and_then(Value::as_str);
        let url = row.get("html_url").and_then(Value::as_str);
        let (number, title, url) = match (number, title, url) {
            (Some(number), Some(title), Some(url)) => (number, title, url),
            _ => continue,
        };
        let author = row
            .get("user")
            .and_then(|user| user.get("login"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        pulls.push(PullRow {
            number,
            title: truncate_chars(title, FIELD_CHARS),
            url: truncate_chars(url, FIELD_CHARS),
            author: truncate_chars(author, FIELD_CHARS),
            merged_at: field(row, "merged_at"),
        });
    }
    Ok(pulls)
}

/// The single-pull body carrying the PR-total additions/deletions.
pub fn parse_pull_detail(body: &str) -> Result<PullDetail, HelperError> {
    let value = parse_json(body)?;
    let row = value
        .as_object()
        .ok_or_else(|| HelperError::MalformedJson("expected a JSON pull object".into()))?;
    Ok(PullDetail {
        additions: row.get("additions").and_then(Value::as_u64).unwrap_or(0),
        deletions: row.get("deletions").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// The changed-files page; rows missing filename are dropped.
pub fn parse_file_rows(body: &str) -> Result<Vec<FileRow>, HelperError> {
    let value = parse_json(body)?;
    let rows = value
        .as_array()
        .ok_or_else(|| HelperError::MalformedJson("expected a JSON array of file rows".into()))?;
    let mut files = Vec::new();
    for row in rows.iter().take(MAX_FILE_ROWS) {
        let Some(path) = row.get("filename").and_then(Value::as_str) else {
            continue;
        };
        files.push(FileRow {
            path: truncate_chars(path, FIELD_CHARS),
            additions: row.get("additions").and_then(Value::as_u64).unwrap_or(0),
            deletions: row.get("deletions").and_then(Value::as_u64).unwrap_or(0),
            status: field(row, "status"),
        });
    }
    Ok(files)
}
