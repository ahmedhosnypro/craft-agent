//! Duplicate-code scanning: the tree walk and line normalization.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One scanned source file; anything past this size is a skip note.
pub const MAX_FILE_BYTES: u64 = 1 << 20;
/// Directory depth; deeper trees get a skip note.
pub const MAX_DEPTH: usize = 48;
/// Source files per root; past the cap the rest of the root is skipped.
pub const MAX_FILES_PER_ROOT: usize = 4_000;
/// Total hashed windows across the scan; past the cap it truncates.
pub const MAX_WINDOWS: usize = 250_000;
/// `--min-lines` ceiling.
pub const MAX_MIN_LINES: usize = 200;
/// `--max-candidates` ceiling.
pub const MAX_CANDIDATES: usize = 500;

const SOURCE_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "java", "c", "h", "cc", "cpp", "hpp", "cs",
    "go", "rb", "php", "swift", "kt", "scala", "sh",
];
const IGNORED_DIRS: &[&str] = &[".git", "target", "node_modules"];

#[derive(Debug, Clone)]
pub struct DupsQuery {
    pub roots: Vec<PathBuf>,
    pub min_lines: usize,
    pub max_candidates: usize,
}

#[derive(Debug, Serialize)]
pub struct DupsReport {
    pub candidates: Vec<Candidate>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub lines: usize,
    pub occurrences: Vec<Occurrence>,
}

#[derive(Debug, Serialize)]
pub struct Occurrence {
    pub root: String,
    pub file: String,
    pub line: usize,
}

pub use crate::dups_scan::scan_dups;

/// A kept line: 1-based original line number and its normalized form.
pub(crate) struct KeptLine {
    pub line: usize,
    pub text: String,
}

/// One source file's normalized lines, root-relative path.
pub(crate) struct ScanFile {
    pub rel: String,
    pub kept: Vec<KeptLine>,
}

/// Normalize one raw line: trim, collapse inner whitespace; blank and
/// pure-comment lines are dropped.
pub(crate) fn normalize_line(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || is_comment(trimmed) {
        return None;
    }
    Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("*/")
}

/// Walk one root in sorted order; every anomaly is a note, never an Err.
pub(crate) fn collect_files(root: &Path, skipped: &mut Vec<String>) -> Vec<ScanFile> {
    let mut files = Vec::new();
    if !root.is_dir() {
        skipped.push(format!("root {}: not a directory, skipped", root.display()));
        return files;
    }
    let mut state = WalkState {
        count: 0,
        capped: false,
    };
    walk(root, root, 0, &mut state, &mut files, skipped);
    files
}

struct WalkState {
    count: usize,
    capped: bool,
}

fn rel_of(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => path.to_string_lossy().to_string(),
    }
}

fn is_source(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, extension)) => {
            let extension = extension.to_ascii_lowercase();
            SOURCE_EXTENSIONS.contains(&extension.as_str())
        }
        None => false,
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    state: &mut WalkState,
    files: &mut Vec<ScanFile>,
    skipped: &mut Vec<String>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        skipped.push(format!(
            "{}: unreadable directory, skipped",
            rel_of(root, dir)
        ));
        return;
    };
    let mut names: Vec<std::ffi::OsString> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
        .collect();
    names.sort();
    for name in names {
        if state.count >= MAX_FILES_PER_ROOT {
            if !state.capped {
                skipped.push("file cap reached; remaining files skipped".to_string());
                state.capped = true;
            }
            return;
        }
        let path = dir.join(&name);
        let name_lossy = name.to_string_lossy();
        if path.is_dir() {
            if IGNORED_DIRS.contains(&name_lossy.as_ref()) {
                continue;
            }
            if depth >= MAX_DEPTH {
                skipped.push(format!(
                    "{}: depth over {MAX_DEPTH}, skipped",
                    rel_of(root, &path)
                ));
                continue;
            }
            walk(root, &path, depth + 1, state, files, skipped);
        } else if path.is_file() && is_source(&name_lossy) {
            state.count += 1;
            scan_path(root, &path, files, skipped);
        }
    }
}

fn scan_path(root: &Path, path: &Path, files: &mut Vec<ScanFile>, skipped: &mut Vec<String>) {
    let rel = rel_of(root, path);
    let Ok(file) = std::fs::File::open(path) else {
        skipped.push(format!("{rel}: unreadable, skipped"));
        return;
    };
    let mut text = String::new();
    if file
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .is_err()
    {
        skipped.push(format!("{rel}: not valid utf-8, skipped"));
        return;
    }
    if text.len() > MAX_FILE_BYTES as usize {
        skipped.push(format!("{rel}: size over {MAX_FILE_BYTES} bytes, skipped"));
        return;
    }
    let mut kept = Vec::new();
    for (offset, raw) in text.lines().enumerate() {
        if let Some(text) = normalize_line(raw) {
            kept.push(KeptLine {
                line: offset + 1,
                text,
            });
        }
    }
    files.push(ScanFile { rel, kept });
}
