// prs corpus: collection over fake routes; a repo failure is a row, never fatal.
#![allow(clippy::unwrap_used)]

mod common;

use common::{FakeFetch, fixture};
use craft_helpers::prs_run::{
    MAX_PAGES, PER_PAGE, PrsQuery, collect_prs, pull_detail_url, pull_files_url, pulls_page_url,
};
use craft_helpers::time::parse_rfc3339_secs;

const PAGE1: &str = "https://api.github.com/repos/storytold/craft-agent/pulls?state=closed&sort=updated&direction=desc&per_page=100&page=1";

fn routes_for(fetch: FakeFetch, org: &str, repo: &str) -> FakeFetch {
    let page = format!(
        "https://api.github.com/repos/{org}/{repo}/pulls?state=closed&sort=updated&direction=desc&per_page=100&page=1"
    );
    let detail = |number: u64| format!("https://api.github.com/repos/{org}/{repo}/pulls/{number}");
    let files =
        |number: u64| format!("https://api.github.com/repos/{org}/{repo}/pulls/{number}/files");
    fetch
        .route(page, 200, fixture("pulls_page1.json"), None)
        .route(detail(101), 200, fixture("pull_101.json"), None)
        .route(files(101), 200, fixture("files_101.json"), None)
        .route(detail(98), 200, fixture("pull_98.json"), None)
        .route(files(98), 200, fixture("files_98.json"), None)
}

fn query(repos: &[&str], lookback: u64, limit: usize, max_prs: usize) -> PrsQuery {
    PrsQuery {
        org: "storytold".to_string(),
        repos: repos.iter().map(|repo| repo.to_string()).collect(),
        now_secs: parse_rfc3339_secs("2026-10-11T12:00:00Z").unwrap(),
        lookback_secs: lookback,
        limit_per_repo: limit,
        max_prs,
        since_label: "48h".to_string(),
    }
}

#[test]
fn urls_match_the_verified_rest_surface() {
    assert_eq!(pulls_page_url("storytold", "craft-agent", 1), PAGE1);
    assert_eq!(
        pull_detail_url("storytold", "craft-agent", 101),
        "https://api.github.com/repos/storytold/craft-agent/pulls/101"
    );
    assert_eq!(
        pull_files_url("storytold", "craft-agent", 101),
        "https://api.github.com/repos/storytold/craft-agent/pulls/101/files"
    );
}

#[test]
fn collects_merged_prs_inside_the_window() {
    let fetch = routes_for(FakeFetch::new(), "storytold", "craft-agent");
    let report = collect_prs(&fetch, &query(&["craft-agent"], 48 * 3600, 30, 50));
    assert!(report.errors.is_empty());
    assert_eq!(report.org, "storytold");
    assert_eq!(report.since, "48h");
    assert_eq!(report.repos.len(), 1);
    let repo = &report.repos[0];
    assert_eq!(repo.repo, "craft-agent");
    let numbers: Vec<u64> = repo.prs.iter().map(|pr| pr.number).collect();
    assert_eq!(numbers, vec![101, 98]);
    let pr = &repo.prs[0];
    assert_eq!(pr.title, "Fix the digest window clamp");
    assert_eq!(pr.author, "dev1");
    assert_eq!(pr.merged_at, "2026-10-11T08:00:00Z");
    assert_eq!(pr.additions, 120);
    assert_eq!(pr.deletions, 30);
    assert_eq!(pr.changed_files.len(), 3);
    assert_eq!(
        pr.changed_files[0].path,
        "crates/craft-digest/src/window.rs"
    );
    assert_eq!(pr.changed_files[0].status, "modified");
}

#[test]
fn failing_repos_become_error_rows_never_fatal() {
    let mut fetch = routes_for(FakeFetch::new(), "storytold", "craft-agent");
    let ratey = "https://api.github.com/repos/storytold/ratey/pulls?state=closed&sort=updated&direction=desc&per_page=100&page=1";
    fetch = fetch.route(
        ratey.to_string(),
        429,
        "[]".to_string(),
        Some("30".to_string()),
    );
    let report = collect_prs(&fetch, &query(&["craft-agent", "ratey"], 48 * 3600, 30, 50));
    assert_eq!(report.repos.len(), 2);
    assert_eq!(report.repos[0].prs.len(), 2);
    assert!(report.repos[1].prs.is_empty());
    assert_eq!(report.errors.len(), 1);
    let row = &report.errors[0];
    assert_eq!(row.repo, "ratey");
    assert!(row.message.contains("429"), "{}", row.message);
    assert!(row.message.contains("30"), "{}", row.message);
}

#[test]
fn malformed_and_unrouted_repos_become_error_rows() {
    let mut fetch = FakeFetch::new();
    let broken = "https://api.github.com/repos/storytold/broken/pulls?state=closed&sort=updated&direction=desc&per_page=100&page=1";
    fetch = fetch.route(broken.to_string(), 200, "{\"broken\": ".to_string(), None);
    let report = collect_prs(&fetch, &query(&["broken", "nolisting"], 48 * 3600, 30, 50));
    assert_eq!(report.repos.len(), 2);
    assert!(report.repos.iter().all(|repo| repo.prs.is_empty()));
    assert_eq!(report.errors.len(), 2);
    assert_eq!(report.errors[0].repo, "broken");
    assert!(report.errors[0].message.contains("JSON"));
    assert_eq!(report.errors[1].repo, "nolisting");
}

fn full_page(first_number: usize) -> String {
    let mut body = String::from('[');
    for offset in 0..PER_PAGE {
        let number = first_number + offset;
        let row = format!(
            "{{\"number\":{number},\"title\":\"t\",\"html_url\":\"u\",\"user\":{{\"login\":\"a\"}},\"merged_at\":null}}"
        );
        body.push_str(&row);
        body.push(',');
    }
    body.pop();
    body.push(']');
    body
}

fn page_url(repo: &str, page: usize) -> String {
    format!(
        "https://api.github.com/repos/storytold/{repo}/pulls?state=closed&sort=updated&direction=desc&per_page=100&page={page}"
    )
}

#[test]
fn pages_beyond_a_full_first_page() {
    let mut fetch = FakeFetch::new().route(page_url("deep", 1), 200, full_page(1000), None);
    fetch = fetch.route(page_url("deep", 2), 200, fixture("pulls_page2.json"), None);
    let detail = "https://api.github.com/repos/storytold/deep/pulls/96";
    let files = "https://api.github.com/repos/storytold/deep/pulls/96/files";
    fetch = fetch.route(detail.to_string(), 200, fixture("pull_96.json"), None);
    fetch = fetch.route(files.to_string(), 200, fixture("files_96.json"), None);
    let report = collect_prs(&fetch, &query(&["deep"], 48 * 3600, 30, 50));
    assert!(report.errors.is_empty());
    assert_eq!(report.repos[0].prs.len(), 1);
    assert_eq!(report.repos[0].prs[0].number, 96);
    assert_eq!(report.repos[0].prs[0].additions, 8);
}

#[test]
fn paging_stops_at_the_page_cap() {
    let mut fetch = FakeFetch::new();
    for page in 1..=MAX_PAGES {
        let body = full_page(1000 + page * 100);
        fetch = fetch.route(page_url("loopy", page), 200, body, None);
    }
    let report = collect_prs(&fetch, &query(&["loopy"], 48 * 3600, 30, 50));
    assert!(report.errors.is_empty());
    assert!(report.repos[0].prs.is_empty());
}

#[test]
fn per_repo_cap_keeps_the_newest_merged() {
    let fetch = routes_for(FakeFetch::new(), "storytold", "craft-agent");
    let report = collect_prs(&fetch, &query(&["craft-agent"], 48 * 3600, 1, 50));
    assert_eq!(report.repos[0].prs.len(), 1);
    assert_eq!(report.repos[0].prs[0].number, 101);
}

#[test]
fn global_cap_truncates_in_repo_order() {
    let mut fetch = routes_for(FakeFetch::new(), "storytold", "one");
    fetch = routes_for(fetch, "storytold", "two");
    let report = collect_prs(&fetch, &query(&["one", "two"], 48 * 3600, 30, 3));
    assert_eq!(report.repos[0].prs.len(), 2);
    assert_eq!(report.repos[1].prs.len(), 1);
    assert!(report.errors.is_empty());
}
