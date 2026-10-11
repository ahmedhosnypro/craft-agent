use craft_digest::bundle::{Bundle, CollectionError, RepoDigest};
use craft_digest::collect::PrRecord;
use craft_digest::jscpd::{CloneSite, DuplicateCandidate};
use craft_digest::render::{DIGEST_CHAR_CAP, render_digest};
use craft_digest::window::{Window, WindowSource};

fn win() -> Window {
    Window {
        since: "2026-10-01".to_string(),
        until: "2026-10-11".to_string(),
        source: WindowSource::Marker,
    }
}

fn pr(number: u64, title: &str) -> PrRecord {
    PrRecord {
        number,
        title: title.to_string(),
        url: format!("https://example.invalid/pr/{number}"),
        author: "dev1".to_string(),
        merged_at: "2026-10-02T00:00:00Z".to_string(),
        additions: 120,
        deletions: 30,
        labels: vec!["bug".to_string()],
        body: "does the thing".to_string(),
    }
}

fn sample() -> Bundle {
    Bundle {
        org: "storytold".to_string(),
        window: win(),
        repos: vec![RepoDigest {
            repo: "photocraft".to_string(),
            merged_prs: vec![pr(101, "Fix parser")],
            agents_excerpt: "Run `just check` before committing.".to_string(),
        }],
        duplicates: vec![DuplicateCandidate {
            first: CloneSite {
                repo: "app-a".to_string(),
                file: "src/lib.rs".to_string(),
                line: 3,
            },
            second: CloneSite {
                repo: "app-b".to_string(),
                file: "src/main.rs".to_string(),
                line: 2,
            },
            tokens: 105,
            lines: 13,
        }],
        errors: vec![CollectionError {
            repo: "bad".to_string(),
            stage: "gh".to_string(),
            message: "rate limited".to_string(),
        }],
    }
}

#[test]
fn renders_all_sections_with_expected_shapes() {
    let out = render_digest(&sample());
    assert!(
        out.contains("# storytold digest 2026-10-01..2026-10-11"),
        "{out}"
    );
    assert!(out.contains("## Merged PRs"));
    assert!(out.contains("### photocraft"));
    assert!(
        out.contains("[#101](https://example.invalid/pr/101) Fix parser - dev1 (+120/-30) `bug`")
    );
    assert!(out.contains("## Cross-repo patterns"));
    assert!(out.contains("## Duplicate-code candidates"));
    let link_a = "https://github.com/storytold/app-a/blob/HEAD/src/lib.rs#L3";
    let link_b = "https://github.com/storytold/app-b/blob/HEAD/src/main.rs#L2";
    assert!(out.contains(&format!("[app-a/src/lib.rs:3]({link_a})")));
    assert!(out.contains(&format!("[app-b/src/main.rs:2]({link_b})")));
    assert!(out.contains("## Collection errors"));
    assert!(out.contains("- collection-error: bad: gh: rate limited"));
    assert!(out.contains("## Usage"));
}

#[test]
fn output_ends_with_the_window_marker() {
    let out = render_digest(&sample());
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[lines.len() - 1],
        "<!-- digest-window: 2026-10-01..2026-10-11 -->"
    );
    assert!(out.ends_with("<!-- digest-window: 2026-10-01..2026-10-11 -->\n"));
}

#[test]
fn pr_bodies_are_framed_as_quoted_data() {
    let mut b = sample();
    let mut p = pr(7, "t");
    p.body = "line one\nline two".to_string();
    b.repos[0].merged_prs.push(p);
    let out = render_digest(&b);
    assert!(out.contains("\n  > line one\n  > line two"));
}

#[test]
fn empty_window_renders_minimal_body() {
    let b = Bundle {
        org: "storytold".to_string(),
        window: win(),
        repos: Vec::new(),
        duplicates: Vec::new(),
        errors: Vec::new(),
    };
    let out = render_digest(&b);
    assert!(out.contains("No merged PRs in this window"));
    assert!(out.ends_with("<!-- digest-window: 2026-10-01..2026-10-11 -->\n"));
    assert!(out.chars().count() < 600);
}

#[test]
fn fallback_window_is_noted_in_the_header() {
    let mut b = sample();
    b.window.source = WindowSource::Fallback;
    let out = render_digest(&b);
    assert!(out.contains("window fallback: 24h"));
}

#[test]
fn oversized_digest_is_capped_with_a_note() {
    let mut b = sample();
    b.repos = (0..40)
        .map(|i| RepoDigest {
            repo: format!("repo{i}"),
            merged_prs: (0..30).map(|n| pr(n, &"long title ".repeat(20))).collect(),
            agents_excerpt: String::new(),
        })
        .collect();
    let out = render_digest(&b);
    assert!(
        out.chars().count() < DIGEST_CHAR_CAP + 200,
        "{}",
        out.chars().count()
    );
    assert!(out.contains("digest truncated"));
    assert!(out.ends_with("<!-- digest-window: 2026-10-01..2026-10-11 -->\n"));
}

#[test]
fn no_error_section_when_no_errors() {
    let mut b = sample();
    b.errors.clear();
    let out = render_digest(&b);
    assert!(!out.contains("## Collection errors"));
    assert!(!out.contains("collection-error"));
}
