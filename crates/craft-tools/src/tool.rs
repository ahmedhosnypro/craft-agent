//! The Tool trait, the per-run ToolCtx, and the bounds every builtin
//! enforces against hostile input.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::envscrub;

use crate::event::ToolStream;
use crate::schema::Schema;

/// Bounds the builtins; defaults are the production caps and the corpora
/// inject tighter ones for fast deterministic runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolLimits {
    pub bash_default_timeout: Duration,
    pub bash_max_timeout: Duration,
    pub proc_grace: Duration,
    pub proc_out_cap: usize,
    pub gh_timeout: Duration,
    pub read_cap: usize,
    pub write_cap: usize,
    pub max_path_bytes: usize,
    pub max_command_bytes: usize,
    pub max_pattern_bytes: usize,
    pub max_args: usize,
    pub glob_max_results: usize,
    pub grep_max_results: usize,
    pub walk_max_entries: usize,
    pub walk_max_depth: usize,
    pub grep_file_cap: usize,
    pub grep_line_cap: usize,
}

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            bash_default_timeout: Duration::from_secs(120),
            bash_max_timeout: Duration::from_secs(600),
            proc_grace: Duration::from_secs(10),
            proc_out_cap: 256 * 1024,
            gh_timeout: Duration::from_secs(60),
            read_cap: 2 * 1024 * 1024,
            write_cap: 2 * 1024 * 1024,
            max_path_bytes: 4096,
            max_command_bytes: 64 * 1024,
            max_pattern_bytes: 8 * 1024,
            max_args: 64,
            glob_max_results: 200,
            grep_max_results: 200,
            walk_max_entries: 20_000,
            walk_max_depth: 32,
            grep_file_cap: 10 * 1024 * 1024,
            grep_line_cap: 500,
        }
    }
}

/// Where a tool child's environment comes from: the real process env in
/// production, an explicit map in the corpora. Either way it is
/// scrubbed per call, never cached.
#[derive(Debug, Clone)]
pub enum EnvSource {
    Process,
    Explicit(Vec<(String, String)>),
}

/// The process environment as UTF-8 pairs; entries with non-UTF-8
/// bytes are dropped rather than fatal.
fn utf8_env_pairs<I>(env: I) -> impl Iterator<Item = (String, String)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    env.into_iter()
        .filter_map(|(k, v)| Some((k.to_str()?.to_string(), v.to_str()?.to_string())))
}

/// Per-run context shared by every tool call: the workspace root, the
/// child-env source, and the read stamps backing edit_file's
/// prior-read and changed-on-disk guards.
#[derive(Debug, Clone)]
pub struct ToolCtx {
    workspace_root: PathBuf,
    env: EnvSource,
    reads: Arc<Mutex<HashMap<PathBuf, u64>>>,
}

impl ToolCtx {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            env: EnvSource::Process,
            reads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_env(workspace_root: PathBuf, env: Vec<(String, String)>) -> Self {
        Self {
            workspace_root,
            env: EnvSource::Explicit(env),
            reads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// The scrubbed environment a tool child receives, computed per call;
    /// non-UTF-8 entries in the process env are dropped, never fatal.
    pub fn child_env(&self) -> Vec<(String, String)> {
        match &self.env {
            EnvSource::Process => envscrub::scrub(utf8_env_pairs(std::env::vars_os())),
            EnvSource::Explicit(pairs) => envscrub::scrub(pairs.iter().cloned()),
        }
    }

    /// A configuration variable from the run's env source (not the
    /// scrubbed child env). The gate knobs are read here; tests drive
    /// them through explicit env pairs instead of the process env.
    pub fn env_value(&self, name: &str) -> Option<String> {
        match &self.env {
            EnvSource::Process => std::env::var(name).ok(),
            EnvSource::Explicit(pairs) => pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone()),
        }
    }

    fn reads(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, u64>> {
        self.reads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn mark_read(&self, path: PathBuf, stamp: u64) {
        self.reads().insert(path, stamp);
    }

    pub fn read_stamp(&self, path: &Path) -> Option<u64> {
        self.reads().get(path).copied()
    }
}

/// A callable tool. `execute` itself never fails; every failure arrives
/// as a typed error inside the returned stream, and the registry wraps
/// the stream with the progress/terminal contract.
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn params_schema(&self) -> Schema;
    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream;
}

#[cfg(test)]
mod tests {
    use super::utf8_env_pairs;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    #[test]
    fn non_utf8_env_entries_are_dropped_not_fatal() {
        let env = vec![
            (OsString::from("KEEP"), OsString::from("yes")),
            (
                OsString::from("BAD_VALUE"),
                OsString::from_vec(vec![0xff, 0xfe]),
            ),
            (OsString::from_vec(vec![0xff]), OsString::from("ok")),
        ];
        let kept: Vec<_> = utf8_env_pairs(env).collect();
        assert_eq!(kept, vec![(String::from("KEEP"), String::from("yes"))]);
    }
}
