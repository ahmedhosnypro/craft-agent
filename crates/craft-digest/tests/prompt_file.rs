// The digest prompt: loads from its default workspace path or an override
// path, and a missing file is a typed error that names the path.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use craft_digest::bundle::Bundle;
use craft_digest::collect::PrRecord;
use craft_digest::error::DigestError;
use craft_digest::prompt::{
    PROMPT_RELATIVE_PATH, build_digest_prompt, default_prompt_path, load_prompt,
};
use craft_digest::window::{Window, WindowSource};

fn tmp_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/tmp/craft-digest-corpus")
        .join(format!("prompt-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn sample_bundle() -> Bundle {
    let mut bundle = Bundle::new(
        String::from("storytold"),
        Window {
            since: String::from("2026-10-01"),
            until: String::from("2026-10-11"),
            source: WindowSource::Marker,
        },
    );
    bundle.push_repo(
        String::from("alpha"),
        vec![PrRecord {
            number: 101,
            title: String::from("Fix parser"),
            url: String::from("https://example.invalid/pr/101"),
            author: String::from("dev1"),
            merged_at: String::from("2026-10-02T12:00:00Z"),
            additions: 12,
            deletions: 3,
            labels: Vec::new(),
            body: String::from("rewrites the parser"),
        }],
    );
    bundle.push_error("beta", "gh", "rate limited");
    bundle
}

#[test]
fn loads_the_template_from_its_default_path() {
    let root = tmp_dir("default");
    let path = default_prompt_path(&root);
    assert_eq!(path, root.join("config/digest-prompt.md"));
    assert_eq!(path, root.join(PROMPT_RELATIVE_PATH));
    write(&path, "TEMPLATE RULES");
    assert_eq!(load_prompt(&path).unwrap(), "TEMPLATE RULES");
}

#[test]
fn loads_an_override_path() {
    let root = tmp_dir("override");
    let path = root.join("prompts/override.md");
    write(&path, "OVERRIDE RULES");
    assert_eq!(load_prompt(&path).unwrap(), "OVERRIDE RULES");
}

#[test]
fn missing_prompt_file_is_a_typed_error_naming_the_path() {
    let path = tmp_dir("missing").join("config/digest-prompt.md");
    let error = load_prompt(&path).unwrap_err();
    assert!(matches!(error, DigestError::Prompt { .. }), "{error}");
    assert!(error.to_string().contains("digest-prompt.md"));
}

#[test]
fn assembled_prompt_carries_template_and_bundle_summary() {
    let root = tmp_dir("assembly");
    let path = default_prompt_path(&root);
    write(&path, "TEMPLATE RULES");
    let template = load_prompt(&path).unwrap();
    let clones = root.join("clones");
    let prompt = build_digest_prompt(&template, &sample_bundle(), &clones);
    assert!(prompt.contains("TEMPLATE RULES"), "{prompt}");
    assert!(prompt.contains("storytold"), "{prompt}");
    assert!(prompt.contains("2026-10-01..2026-10-11"), "{prompt}");
    assert!(prompt.contains("alpha"), "{prompt}");
    assert!(
        prompt.contains("https://example.invalid/pr/101"),
        "{prompt}"
    );
    assert!(prompt.contains("collection-error"), "{prompt}");
    assert!(
        prompt.contains(clones.to_string_lossy().as_ref()),
        "{prompt}"
    );
}
