// Corpus fixtures: unwrap() is the loud-test idiom here.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// The fake-gh tests write a script and exec it immediately; a parallel
/// fork in another test can inherit the still-open write fd and fail the
/// exec with ETXTBSY, so they serialize.
static SCRIPT_LOCK: Mutex<()> = Mutex::new(());

fn script_guard() -> MutexGuard<'static, ()> {
    SCRIPT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

use craft_digest::collect::{
    GhRunner, PR_BODY_PREFIX_BYTES, PrRecord, collect_all, parse_pr_rows, parse_repo_list,
};
use craft_digest::window::{Window, WindowSource};

fn win() -> Window {
    Window {
        since: "2026-10-01".to_string(),
        until: "2026-10-11".to_string(),
        source: WindowSource::Marker,
    }
}

const GH_ROWS: &str = r#"[
{"number":101,"title":"Fix parser","url":"https://example.invalid/pr/101","author":{"login":"dev1"},"mergedAt":"2026-10-01T12:00:00Z","additions":120,"deletions":30,"labels":[{"name":"bug"}],"body":"body 101"},
{"number":102,"title":"Add CLI","url":"https://example.invalid/pr/102","author":{"login":"dev2"},"mergedAt":"2026-10-02T12:00:00Z","additions":10,"deletions":5,"labels":[{"name":"feature"}],"body":"body 102"},
{"number":103,"title":"Docs","url":"https://example.invalid/pr/103","author":{"login":"dev3"},"mergedAt":"2026-10-03T12:00:00Z","additions":3,"deletions":1,"labels":[],"body":"body 103"}
]"#;

#[test]
fn parses_all_pr_row_fields() {
    let (prs, raw_rows) = parse_pr_rows(GH_ROWS).unwrap();
    assert_eq!(raw_rows, 3);
    assert_eq!(prs.len(), 3);

    let first: &PrRecord = &prs[0];
    assert_eq!(first.number, 101);
    assert_eq!(first.title, "Fix parser");
    assert_eq!(first.url, "https://example.invalid/pr/101");
    assert_eq!(first.author, "dev1");
    assert_eq!(first.merged_at, "2026-10-01T12:00:00Z");
    assert_eq!(first.additions, 120);
    assert_eq!(first.deletions, 30);
    assert_eq!(first.labels, vec!["bug".to_string()]);
    assert_eq!(first.body, "body 101");

    assert_eq!(prs[2].labels, Vec::<String>::new());
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/craft-digest-corpus")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fake_gh(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("gh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn collect_all_gathers_prs_per_repo() {
    let _guard = script_guard();
    let dir = tmp_dir("collect-ok");
    let gh = fake_gh(&dir, &format!("echo '{GH_ROWS}'"));
    let runner = GhRunner::with_program(gh);
    let repos = vec!["alpha".to_string(), "beta".to_string()];
    let bundle = collect_all(&runner, "storytold", &repos, &win());
    assert_eq!(bundle.org, "storytold");
    assert!(bundle.errors.is_empty());
    assert_eq!(bundle.repos.len(), 2);
    for repo in &bundle.repos {
        assert_eq!(repo.merged_prs.len(), 3);
        assert_eq!(repo.merged_prs[0].body, "body 103");
    }
}

#[test]
fn failing_repo_records_error_row_and_continues() {
    let _guard = script_guard();
    let dir = tmp_dir("collect-fail");
    let gh = fake_gh(
        &dir,
        r#"case "$*" in
  *bad*) echo "gh: API rate limit exceeded (HTTP 403)" >&2; exit 1 ;;
  *) echo '[]' ;;
esac"#,
    );
    let runner = GhRunner::with_program(gh);
    let repos = vec!["good".to_string(), "bad".to_string()];
    let bundle = collect_all(&runner, "storytold", &repos, &win());
    assert_eq!(bundle.repos.len(), 2);
    assert_eq!(bundle.repos[1].merged_prs.len(), 0);
    assert_eq!(bundle.repos[0].merged_prs.len(), 0);
    assert_eq!(bundle.errors.len(), 1);
    let err = &bundle.errors[0];
    assert_eq!(err.repo, "bad");
    assert_eq!(err.stage, "gh");
    assert!(err.message.contains("rate limit"));
}

#[test]
fn unspawnable_gh_records_error_row() {
    let _guard = script_guard();
    let dir = tmp_dir("collect-spawn");
    let runner = GhRunner::with_program(dir.join("missing-gh"));
    let bundle = collect_all(&runner, "storytold", &["alpha".to_string()], &win());
    assert_eq!(bundle.repos.len(), 1);
    assert_eq!(bundle.errors.len(), 1);
    assert_eq!(bundle.errors[0].repo, "alpha");
    assert_eq!(bundle.errors[0].stage, "gh");
}

#[test]
fn non_json_gh_output_records_error_row() {
    let _guard = script_guard();
    let dir = tmp_dir("collect-badjson");
    let gh = fake_gh(&dir, "echo 'not json at all'");
    let runner = GhRunner::with_program(gh);
    let bundle = collect_all(&runner, "storytold", &["alpha".to_string()], &win());
    assert_eq!(bundle.repos.len(), 1);
    assert_eq!(bundle.errors.len(), 1);
    assert_eq!(bundle.errors[0].stage, "gh-output");
}

#[test]
fn repo_list_skips_comments_and_blanks() {
    let text = "# comment\n\nphotocraft\n\n# mid\nwordcraft\n";
    let repos = parse_repo_list(text);
    assert_eq!(repos, vec!["photocraft", "wordcraft"]);
}

#[test]
fn pr_body_is_truncated_to_bounded_prefix() {
    let long = "x".repeat(5000);
    let json = format!(
        r#"[{{"number":1,"title":"t","url":"u","author":null,"mergedAt":"2026-10-02T00:00:00Z","additions":1,"deletions":0,"labels":null,"body":"{long}"}}]"#
    );
    let (prs, _) = parse_pr_rows(&json).unwrap();
    assert!(prs[0].body.len() <= PR_BODY_PREFIX_BYTES);
    assert_eq!(prs[0].body, long[..PR_BODY_PREFIX_BYTES]);

    let wide = "é".repeat(1500);
    let json = format!(
        r#"[{{"number":2,"title":"t","url":"u","mergedAt":"2026-10-02T00:00:00Z","body":"{wide}"}}]"#
    );
    let (prs, _) = parse_pr_rows(&json).unwrap();
    assert_eq!(prs[0].body.chars().count(), 1000);
}

#[test]
fn hostile_rows_fall_back_to_safe_values() {
    let json = r#"[{"number":8,"title":"ok","url":"u","author":null,"mergedAt":"2026-10-03T00:00:00Z","additions":null,"deletions":null,"labels":null,"body":null}]"#;
    let (prs, _) = parse_pr_rows(json).unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].number, 8);
    assert_eq!(prs[0].author, "unknown");
    assert_eq!(prs[0].additions, 0);
    assert_eq!(prs[0].deletions, 0);
    assert_eq!(prs[0].labels, Vec::<String>::new());
    assert_eq!(prs[0].body, "");
}

#[test]
fn rows_missing_essentials_are_dropped() {
    let json = r#"[
 {"title":"no number","url":"u","mergedAt":"2026-10-02T00:00:00Z"},
 {"number":"nine","title":"string number","url":"u","mergedAt":"2026-10-02T00:00:00Z"},
 {"number":10,"mergedAt":"2026-10-02T00:00:00Z"},
 {"number":11,"title":"no url","mergedAt":"2026-10-02T00:00:00Z"},
 {"number":12,"title":"no mergedAt","url":"u"}
]"#;
    let (prs, _) = parse_pr_rows(json).unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].number, 12);
    assert_eq!(prs[0].title, "no mergedAt");
}

#[test]
fn window_filter_drops_out_of_range_and_garbage_dates() {
    let _guard = script_guard();
    let json = r#"[
 {"number":1,"title":"before","url":"u","mergedAt":"2026-09-30T23:00:00Z"},
 {"number":2,"title":"on-since","url":"u","mergedAt":"2026-10-01T00:00:00Z"},
 {"number":3,"title":"on-until","url":"u","mergedAt":"2026-10-11T23:00:00Z"},
 {"number":4,"title":"after","url":"u","mergedAt":"2026-10-12T00:00:00Z"},
 {"number":5,"title":"garbage","url":"u","mergedAt":"not-a-date"}
]"#;
    let dir = tmp_dir("collect-window");
    let gh = fake_gh(&dir, &format!("echo '{json}'"));
    let bundle = collect_all(
        &GhRunner::with_program(gh),
        "storytold",
        &["alpha".to_string()],
        &win(),
    );
    let prs = &bundle.repos[0].merged_prs;
    let titles: Vec<&str> = prs.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, vec!["on-until", "on-since"]);
}

#[test]
fn per_repo_cap_keeps_newest_thirty() {
    let _guard = script_guard();
    let rows: Vec<String> = (1..=40)
        .map(|n| {
            let (day, hour) = (1 + n / 8, n % 8);
            format!(
                r#"{{"number":{n},"title":"pr {n}","url":"u","mergedAt":"2026-10-{day:02}T{hour:02}:00:00Z","body":""}}"#
            )
        })
        .collect();
    let json = format!("[{}]", rows.join(","));
    let dir = tmp_dir("collect-newest");
    let gh = fake_gh(&dir, &format!("echo '{json}'"));
    let bundle = collect_all(
        &GhRunner::with_program(gh),
        "storytold",
        &["alpha".to_string()],
        &win(),
    );
    let prs = &bundle.repos[0].merged_prs;
    assert_eq!(prs.len(), 30);
    assert_eq!(prs[0].number, 40);
    assert_eq!(prs[29].number, 11);
}

#[test]
fn pr_list_cap_binding_records_error_row() {
    let _guard = script_guard();
    let rows: Vec<String> = (1..=201)
        .map(|n| {
            format!(
                r#"{{"number":{n},"title":"t","url":"u","mergedAt":"2026-10-05T00:00:00Z","body":""}}"#
            )
        })
        .collect();
    let json = format!("[{}]", rows.join(","));
    let dir = tmp_dir("collect-cap");
    let gh = fake_gh(&dir, &format!("echo '{json}'"));
    let bundle = collect_all(
        &GhRunner::with_program(gh),
        "storytold",
        &["alpha".to_string()],
        &win(),
    );
    assert_eq!(bundle.repos[0].merged_prs.len(), 30);
    assert_eq!(bundle.errors.len(), 1);
    assert_eq!(bundle.errors[0].stage, "pr-list");
}

#[test]
fn collected_text_is_sanitized_into_the_bundle() {
    let json = concat!(
        r#"[{"number":1,"title":"vi"#,
        r"\u0007",
        r#"sible","url":"u","mergedAt":"2026-10-02T00:00:00Z","body":"<!-- hidden --> show"}]"#
    );
    let (prs, _) = parse_pr_rows(json).unwrap();
    assert_eq!(prs[0].title, "visible");
    assert_eq!(prs[0].body, " show");
}

#[test]
fn gh_call_deadline_records_error_row() {
    let _guard = script_guard();
    let dir = tmp_dir("collect-deadline");
    let gh = fake_gh(&dir, "sleep 30");
    let runner = GhRunner::with_program(gh).with_deadline(Duration::from_millis(200));
    let bundle = collect_all(&runner, "storytold", &["alpha".to_string()], &win());
    assert_eq!(bundle.repos.len(), 1);
    assert_eq!(bundle.repos[0].merged_prs.len(), 0);
    assert_eq!(bundle.errors.len(), 1);
    assert_eq!(bundle.errors[0].repo, "alpha");
    assert_eq!(bundle.errors[0].stage, "gh");
    assert!(bundle.errors[0].message.contains("deadline"));
}

#[test]
fn full_page_with_dropped_rows_still_records_the_cap() {
    let _guard = script_guard();
    let rows: Vec<String> = (1..=201)
        .map(|n| {
            if n % 10 == 0 {
                r#"{"number":9999,"title":"no url"}"#.to_string()
            } else {
                format!(
                    r#"{{"number":{n},"title":"t","url":"u","mergedAt":"2026-10-05T00:00:00Z","body":""}}"#
                )
            }
        })
        .collect();
    let json = format!("[{}]", rows.join(","));
    let dir = tmp_dir("collect-cap-dropped");
    let gh = fake_gh(&dir, &format!("echo '{json}'"));
    let bundle = collect_all(
        &GhRunner::with_program(gh),
        "storytold",
        &["alpha".to_string()],
        &win(),
    );
    assert_eq!(bundle.repos[0].merged_prs.len(), 30);
    assert_eq!(bundle.errors.len(), 1);
    assert_eq!(bundle.errors[0].stage, "pr-list");
}
