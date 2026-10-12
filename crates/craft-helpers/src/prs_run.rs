//! Collection over the pulls REST surface; a repo failure is a row.

use serde::Serialize;

use crate::Truncation;
use crate::error::{HelperError, bounded};
use crate::fetch::{Fetch, FetchResponse};
use crate::prs::{FileRow, PullRow, parse_file_rows, parse_pull_detail, parse_pull_page};
use crate::time::parse_rfc3339_secs;

pub const API_BASE: &str = "https://api.github.com";
pub const PER_PAGE: usize = 100;
/// Pages fetched per repository, however full they come back.
pub const MAX_PAGES: usize = 5;
/// `--limit-per-repo` ceiling.
pub const MAX_LIMIT_PER_REPO: usize = 100;
/// `--max-prs` ceiling.
pub const MAX_MAX_PRS: usize = 500;
pub const TOKEN_ENV: &str = "GH_TOKEN";

pub fn pulls_page_url(org: &str, repo: &str, page: usize) -> String {
    format!(
        "{API_BASE}/repos/{org}/{repo}/pulls?state=closed&sort=updated&direction=desc&per_page={PER_PAGE}&page={page}"
    )
}

pub fn pull_detail_url(org: &str, repo: &str, number: u64) -> String {
    format!("{API_BASE}/repos/{org}/{repo}/pulls/{number}")
}

pub fn pull_files_url(org: &str, repo: &str, number: u64) -> String {
    format!("{API_BASE}/repos/{org}/{repo}/pulls/{number}/files")
}

/// The bearer token from an injected environment; missing or blank is a
/// typed error naming the variable. The value itself is never echoed.
pub fn resolve_token(lookup: impl Fn(&str) -> Option<String>) -> Result<String, HelperError> {
    let token = lookup(TOKEN_ENV).ok_or(HelperError::MissingToken)?;
    let token = token.trim();
    if token.is_empty() {
        return Err(HelperError::MissingToken);
    }
    Ok(token.to_string())
}

#[derive(Debug, Clone)]
pub struct PrsQuery {
    pub org: String,
    pub repos: Vec<String>,
    pub now_secs: u64,
    pub lookback_secs: u64,
    pub limit_per_repo: usize,
    pub max_prs: usize,
    pub since_label: String,
}

#[derive(Debug, Serialize)]
pub struct PrsReport {
    pub org: String,
    pub since: String,
    pub repos: Vec<RepoReport>,
    pub errors: Vec<ErrorRow>,
    pub truncated: Vec<Truncation>,
}

#[derive(Debug, Serialize)]
pub struct RepoReport {
    pub repo: String,
    pub prs: Vec<PrsEntry>,
}

#[derive(Debug, Serialize)]
pub struct PrsEntry {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: String,
    pub merged_at: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: Vec<FileRow>,
}

#[derive(Debug, Serialize)]
pub struct ErrorRow {
    pub repo: String,
    pub message: String,
}

/// Collect every repo; failures become error rows, never a fatal Err.
pub fn collect_prs(fetch: &dyn Fetch, query: &PrsQuery) -> PrsReport {
    let cutoff = query.now_secs.saturating_sub(query.lookback_secs);
    let mut report = PrsReport {
        org: query.org.clone(),
        since: query.since_label.clone(),
        repos: Vec::new(),
        errors: Vec::new(),
        truncated: Vec::new(),
    };
    let mut budget = query.max_prs;
    let mut truncated: Vec<Truncation> = Vec::new();
    for repo in &query.repos {
        let (prs, messages) = collect_repo(fetch, query, repo, cutoff, &mut budget, &mut truncated);
        for message in messages {
            report.errors.push(ErrorRow::new(repo.clone(), &message));
        }
        report.repos.push(RepoReport {
            repo: repo.clone(),
            prs,
        });
    }
    report.truncated = truncated;
    report
}

impl ErrorRow {
    fn new(repo: String, message: &str) -> Self {
        ErrorRow {
            repo,
            message: bounded(message),
        }
    }
}

fn ensure_ok(response: &FetchResponse) -> Result<(), HelperError> {
    if response.status == 403 || response.status == 429 {
        return Err(HelperError::RateLimited {
            status: response.status,
            retry_after: response.retry_after.clone(),
        });
    }
    if !(200..300).contains(&response.status) {
        return Err(HelperError::Http(format!("HTTP {}", response.status)));
    }
    Ok(())
}

fn collect_repo(
    fetch: &dyn Fetch,
    query: &PrsQuery,
    repo: &str,
    cutoff: u64,
    budget: &mut usize,
    truncated: &mut Vec<Truncation>,
) -> (Vec<PrsEntry>, Vec<String>) {
    let mut matched: Vec<(u64, PullRow)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut more_may_exist = false;
    for page in 1..=MAX_PAGES {
        let url = pulls_page_url(&query.org, repo, page);
        let response = match fetch.get(&url) {
            Ok(response) => response,
            Err(err) => {
                errors.push(err.to_string());
                return (Vec::new(), errors);
            }
        };
        let row = match ensure_ok(&response).and_then(|()| parse_pull_page(&response.body)) {
            Ok(rows) => rows,
            Err(err) => {
                errors.push(err.to_string());
                return (Vec::new(), errors);
            }
        };
        let page_len = row.len();
        let mut clipped_on_page = false;
        for pull in row {
            if matched.len() >= query.limit_per_repo {
                clipped_on_page = true;
                break;
            }
            if let Some(secs) = parse_rfc3339_secs(&pull.merged_at)
                && secs >= cutoff
            {
                matched.push((secs, pull));
            }
        }
        more_may_exist = clipped_on_page || page_len == PER_PAGE;
        if matched.len() >= query.limit_per_repo || page_len < PER_PAGE {
            break;
        }
    }
    // Sort by the parsed instant: offset-form timestamps order wrongly as text.
    matched.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.number.cmp(&a.1.number)));
    matched.truncate(query.limit_per_repo);
    if matched.len() >= query.limit_per_repo && more_may_exist {
        truncated.push(Truncation::new(
            "prs-per-repo",
            query.limit_per_repo,
            &format!(
                "{repo}: more merged PRs may exist; raise --limit-per-repo (ceiling {MAX_LIMIT_PER_REPO})"
            ),
        ));
    }
    let mut prs = Vec::new();
    let matched_count = matched.len();
    for (_, pull) in matched {
        if *budget == 0 {
            break;
        }
        match fetch_entry(fetch, query, repo, &pull) {
            Ok(entry) => {
                prs.push(entry);
                *budget -= 1;
            }
            Err(err @ HelperError::RateLimited { .. }) => {
                errors.push(err.to_string());
                break;
            }
            Err(err) => errors.push(err.to_string()),
        }
    }
    if *budget == 0 && prs.len() < matched_count {
        truncated.push(Truncation::new(
            "prs-total",
            query.max_prs,
            &format!("{repo}: total cap reached; raise --max-prs (ceiling {MAX_MAX_PRS})"),
        ));
    }
    (prs, errors)
}

/// The PR-total additions/deletions come from the pull detail; the
/// changed-files list is the first REST page, capped per file row.
fn fetch_entry(
    fetch: &dyn Fetch,
    query: &PrsQuery,
    repo: &str,
    pull: &PullRow,
) -> Result<PrsEntry, HelperError> {
    let detail = fetch
        .get(&pull_detail_url(&query.org, repo, pull.number))
        .and_then(|response| ensure_ok(&response).map(|()| response))?;
    let detail = parse_pull_detail(&detail.body)?;
    let files = fetch
        .get(&pull_files_url(&query.org, repo, pull.number))
        .and_then(|response| ensure_ok(&response).map(|()| response))?;
    let changed_files = parse_file_rows(&files.body)?;
    Ok(PrsEntry {
        number: pull.number,
        title: pull.title.clone(),
        url: pull.url.clone(),
        author: pull.author.clone(),
        merged_at: pull.merged_at.clone(),
        additions: detail.additions,
        deletions: detail.deletions,
        changed_files,
    })
}
