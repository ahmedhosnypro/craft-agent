//! Window hashing: the scan half of duplicate detection.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::Truncation;
use crate::dups::{
    DupsQuery, DupsReport, MAX_CANDIDATES, MAX_MIN_LINES, MAX_WINDOWS, ScanFile, collect_files,
};
use crate::dups_rank::rank_groups;
use crate::error::HelperError;
use crate::error::invalid_bound;

pub(crate) struct FileRef {
    pub(crate) root: String,
    pub(crate) file: ScanFile,
}

#[derive(Clone, Copy)]
pub(crate) struct Hit {
    pub(crate) file: usize,
    pub(crate) index: usize,
}

/// Hash every window of `min_lines` kept lines; the caller reports groups
/// that repeat across files or disjointly inside one file.
pub fn scan_dups(query: &DupsQuery) -> Result<DupsReport, HelperError> {
    if !(2..=MAX_MIN_LINES).contains(&query.min_lines) {
        return Err(invalid_bound(
            "min-lines",
            &query.min_lines.to_string(),
            2,
            MAX_MIN_LINES,
        ));
    }
    if query.max_candidates == 0 || query.max_candidates > MAX_CANDIDATES {
        return Err(invalid_bound(
            "max-candidates",
            &query.max_candidates.to_string(),
            1,
            MAX_CANDIDATES,
        ));
    }
    let mut skipped: Vec<String> = Vec::new();
    let mut truncated: Vec<Truncation> = Vec::new();
    let mut files: Vec<FileRef> = Vec::new();
    for root in &query.roots {
        let label = root.to_string_lossy().to_string();
        for file in collect_files(root, &mut skipped, &mut truncated) {
            files.push(FileRef {
                root: label.clone(),
                file,
            });
        }
    }
    let mut windows: HashMap<u64, Vec<Hit>> = HashMap::new();
    if let Some(root) = hash_windows(&files, query.min_lines, &mut windows) {
        skipped.push(format!(
            "window cap ({MAX_WINDOWS}) reached during root {root}; remaining files skipped"
        ));
        truncated.push(Truncation::new(
            "windows",
            MAX_WINDOWS,
            &format!("root {root} filled the window cap; scan narrower roots"),
        ));
    }
    let candidates = rank_groups(
        &files,
        &windows,
        query.min_lines,
        query.max_candidates,
        query.cross_only,
        &mut truncated,
    );
    Ok(DupsReport {
        candidates,
        skipped,
        truncated,
    })
}

/// Hash every window; returns the root whose scan was in progress when the
/// window cap fired, so the skip note can name it.
fn hash_windows(
    files: &[FileRef],
    min_lines: usize,
    windows: &mut HashMap<u64, Vec<Hit>>,
) -> Option<String> {
    let mut total: usize = 0;
    for (file_index, record) in files.iter().enumerate() {
        let kept = &record.file.kept;
        if kept.len() < min_lines {
            continue;
        }
        for start in 0..=kept.len() - min_lines {
            if total >= MAX_WINDOWS {
                return Some(record.root.clone());
            }
            let mut hasher = DefaultHasher::new();
            for line in &kept[start..start + min_lines] {
                line.text.hash(&mut hasher);
            }
            let hash = hasher.finish();
            windows.entry(hash).or_default().push(Hit {
                file: file_index,
                index: start,
            });
            total += 1;
        }
    }
    None
}
