//! jscpd v5 report parsing: recorded reports into bounded clone candidates.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde_json::Value;

use crate::error::{DigestError, bounded};

/// Cross-repo clone pairs kept in the bundle, ordered by duplicated tokens.
pub const MAX_DUPLICATE_CANDIDATES: usize = 20;
/// Reports larger than this are rejected before parsing.
pub const JSCPD_REPORT_CAP_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneSite {
    pub repo: String,
    pub file: String,
    pub line: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCandidate {
    pub first: CloneSite,
    pub second: CloneSite,
    pub tokens: u64,
    pub lines: u64,
}

/// Read a report from disk, byte cap applied before allocating.
pub fn read_jscpd_report(path: &Path) -> Result<Vec<DuplicateCandidate>, DigestError> {
    let file = File::open(path).map_err(|err| DigestError::Jscpd(bounded(&err.to_string())))?;
    let mut bytes = Vec::new();
    file.take((JSCPD_REPORT_CAP_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|err| DigestError::Jscpd(bounded(&err.to_string())))?;
    if bytes.len() > JSCPD_REPORT_CAP_BYTES {
        return Err(DigestError::JscpdTooLarge(JSCPD_REPORT_CAP_BYTES));
    }
    parse_jscpd_report(&String::from_utf8_lossy(&bytes))
}

/// Top pairs by duplicated tokens; malformed entries are skipped, not fatal.
pub fn parse_jscpd_report(text: &str) -> Result<Vec<DuplicateCandidate>, DigestError> {
    if text.len() > JSCPD_REPORT_CAP_BYTES {
        return Err(DigestError::JscpdTooLarge(JSCPD_REPORT_CAP_BYTES));
    }
    let value: Value =
        serde_json::from_str(text).map_err(|err| DigestError::Jscpd(bounded(&err.to_string())))?;
    let duplicates = value
        .get("duplicates")
        .and_then(Value::as_array)
        .ok_or_else(|| DigestError::Jscpd(bounded("missing the duplicates array")))?;
    let mut candidates = Vec::new();
    for entry in duplicates.iter() {
        if let Some(candidate) = parse_entry(entry) {
            candidates.push(candidate);
        }
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.tokens));
    candidates.truncate(MAX_DUPLICATE_CANDIDATES);
    Ok(candidates)
}

fn parse_entry(entry: &Value) -> Option<DuplicateCandidate> {
    let tokens = entry.get("tokens")?.as_u64()?;
    let lines = entry.get("lines").and_then(Value::as_u64).unwrap_or(0);
    let first = entry.get("firstFile").and_then(parse_site)?;
    let second = entry.get("secondFile").and_then(parse_site)?;
    Some(DuplicateCandidate {
        first,
        second,
        tokens,
        lines,
    })
}

/// File names are scan-root-relative: the first path segment is the repo.
fn parse_site(site: &Value) -> Option<CloneSite> {
    let name = site.get("name")?.as_str()?;
    let (repo, file) = name.split_once('/')?;
    let line = site.get("start").and_then(Value::as_u64).unwrap_or(0);
    Some(CloneSite {
        repo: repo.to_string(),
        file: file.to_string(),
        line,
    })
}
