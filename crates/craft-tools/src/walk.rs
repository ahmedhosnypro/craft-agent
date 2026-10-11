//! A bounded, deterministic directory walker shared by glob and grep.
//! Entries are visited in sorted name order; symlinks are never
//! followed; every loop is capped.

use std::path::{Path, PathBuf};

use crate::error::ToolError;
use crate::pathguard::sensitive_reason;

/// Directory names never traversed. Kept in one place so the skip set
/// is identical for glob and grep.
pub const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules"];

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WalkOutcome {
    pub entries_seen: usize,
    pub visited_files: usize,
    pub skipped_sensitive: usize,
    pub skipped_unreadable: usize,
}

/// Walk `root` depth-first, visiting files in sorted name order. The
/// visitor returns false to stop early (result caps). Sensitive files
/// are skipped and counted, never visited.
pub fn walk_files(
    root: &Path,
    max_entries: usize,
    max_depth: usize,
    visit: &mut dyn FnMut(PathBuf) -> bool,
) -> Result<WalkOutcome, ToolError> {
    let mut outcome = WalkOutcome::default();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => {
                outcome.skipped_unreadable = outcome.skipped_unreadable.saturating_add(1);
                continue;
            }
        };
        let mut listed: Vec<(PathBuf, bool)> = Vec::new();
        for entry in entries {
            let Ok(entry) = entry else {
                outcome.skipped_unreadable = outcome.skipped_unreadable.saturating_add(1);
                continue;
            };
            outcome.entries_seen = outcome.entries_seen.saturating_add(1);
            if outcome.entries_seen > max_entries {
                return Ok(outcome);
            }
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            listed.push((entry.path(), is_dir));
        }
        listed.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
        for (path, is_dir) in listed {
            if is_dir {
                let descend = depth < max_depth
                    && !is_skipped_dir(&path)
                    && sensitive_reason(&path).is_none();
                if descend {
                    stack.push((path, depth + 1));
                }
            } else {
                if sensitive_reason(&path).is_some() {
                    outcome.skipped_sensitive = outcome.skipped_sensitive.saturating_add(1);
                    continue;
                }
                outcome.visited_files = outcome.visited_files.saturating_add(1);
                if !visit(path) {
                    return Ok(outcome);
                }
            }
        }
    }
    Ok(outcome)
}

fn is_skipped_dir(path: &Path) -> bool {
    path.file_name()
        .map(|n| {
            let name = n.to_string_lossy();
            SKIP_DIRS.iter().any(|skip| *skip == name)
        })
        .unwrap_or(false)
}
