// Shared TEST_BYOK_* loader for the live corpora; each corpus binary mounts
// this file via #[path]. Names are printable; values never are.

use std::path::{Path, PathBuf};

pub struct TestKeys {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

// Walk up at most 3 levels from the crate manifest to find a repo .env.
fn env_file_candidates(manifest: &Path) -> Vec<PathBuf> {
    let mut out = vec![manifest.join(".env")];
    let mut dir = Some(manifest);
    for _ in 0..3 {
        let Some(parent) = dir.and_then(Path::parent) else {
            break;
        };
        out.push(parent.join(".env"));
        dir = Some(parent);
    }
    out
}

// Minimal KEY=VALUE lines: trims, skips blanks and #-comments, strips quotes.
fn parse_env_file(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            continue;
        }
        let unquoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
        let value = unquoted.unwrap_or(value);
        out.push((key.to_string(), value.to_string()));
    }
    out
}

fn file_value(var: &str) -> Option<String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for path in env_file_candidates(&manifest) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (key, value) in parse_env_file(&text) {
            if key == var {
                return Some(value);
            }
        }
    }
    None
}

// Process env wins; a set value (even empty) is final.
fn read_var(var: &str) -> Option<String> {
    match std::env::var(var) {
        Ok(value) => Some(value),
        Err(_) => file_value(var),
    }
}

// The full trio, or None when any of the three is missing or empty.
pub fn test_keys() -> Option<TestKeys> {
    let base_url = read_var("TEST_BYOK_BASE_URL").filter(|v| !v.is_empty())?;
    let api_key = read_var("TEST_BYOK_API_KEY").filter(|v| !v.is_empty())?;
    let model = read_var("TEST_BYOK_MODEL").filter(|v| !v.is_empty())?;
    Some(TestKeys {
        base_url,
        api_key,
        model,
    })
}

// Yields TestKeys, or prints the SKIP line and early-returns Ok from the test.
#[macro_export]
macro_rules! require_keys {
    ($corpus:expr) => {
        match $crate::byok_env::test_keys() {
            Some(keys) => keys,
            None => {
                eprintln!(
                    "SKIP: TEST_BYOK_* are not set (no .env) — the live {} corpus is off",
                    $corpus
                );
                return Ok(());
            }
        }
    };
}
