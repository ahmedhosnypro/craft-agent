// Corpus fixtures: unwrap() is the loud-test idiom here.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use craft_digest::bundle::Bundle;
use craft_digest::collect::PrRecord;
use craft_digest::dry_run::emit_dry_run;
use craft_digest::render::render_digest;
use craft_digest::scrub::scrub;
use craft_digest::window::{Window, WindowSource};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/craft-digest-corpus")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn bundle_with_leak() -> (Bundle, String) {
    let token = format!("ghp_{}", "k".repeat(36));
    let b = Bundle {
        org: "storytold".to_string(),
        window: Window {
            since: "2026-10-01".to_string(),
            until: "2026-10-11".to_string(),
            source: WindowSource::Marker,
        },
        repos: vec![craft_digest::bundle::RepoDigest {
            repo: "photocraft".to_string(),
            merged_prs: vec![PrRecord {
                number: 1,
                title: "Fix parser".to_string(),
                url: "https://example.invalid/pr/1".to_string(),
                author: "dev1".to_string(),
                merged_at: "2026-10-02T00:00:00Z".to_string(),
                additions: 1,
                deletions: 0,
                labels: Vec::new(),
                body: format!("rotated in a paste: {token}"),
            }],
            agents_excerpt: String::new(),
        }],
        duplicates: Vec::new(),
        errors: Vec::new(),
    };
    (b, token)
}

#[test]
fn dry_run_renders_scrubbed_digest_to_the_writer() {
    let (b, token) = bundle_with_leak();
    let mut out = Vec::new();
    emit_dry_run(&mut out, &b).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text, scrub(&render_digest(&b)));
    assert!(!text.contains(&token));
    assert!(text.ends_with("<!-- digest-window: 2026-10-01..2026-10-11 -->\n"));
}

#[test]
fn dry_run_writes_no_files() {
    let dir = tmp_dir("dry-run");
    fs::write(dir.join("digest.md"), "sentinel").unwrap();
    let (b, _) = bundle_with_leak();
    let mut out = Vec::new();
    emit_dry_run(&mut out, &b).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("digest.md")).unwrap(),
        "sentinel"
    );
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}
