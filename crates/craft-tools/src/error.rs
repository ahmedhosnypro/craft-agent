//! The crate's closed tool-error taxonomy. Every fallible path returns
//! one of these; nothing panics.

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolError {
    #[error("invalid arguments for tool `{tool}`: {msg}")]
    InvalidArgs { tool: String, msg: String },
    #[error("unknown tool `{name}`")]
    UnknownTool { name: String },
    #[error("path rejected ({reason}): {path}")]
    SensitivePath { path: String, reason: &'static str },
    #[error("path escapes the workspace root: {path}")]
    EscapesWorkspace { path: String },
    #[error("file not found: {path}")]
    NotFound { path: String },
    #[error("file exceeds the read limit ({size} > {limit} bytes): {path}")]
    FileTooLarge {
        path: String,
        size: u64,
        limit: usize,
    },
    #[error("content exceeds the write limit ({size} > {limit} bytes)")]
    WriteTooLarge { size: usize, limit: usize },
    #[error("file must be read before editing: {path}")]
    NotReadFirst { path: String },
    #[error("file changed on disk since it was last read: {path}")]
    ChangedOnDisk { path: String },
    #[error("old_string not found in {path}")]
    EditNoMatch { path: String },
    #[error("old_string matches {count} locations in {path}; add context or pass replace_all")]
    EditAmbiguous { path: String, count: usize },
    #[error("command exceeded its {timeout_ms}ms timeout and was terminated")]
    TimedOut { timeout_ms: u64 },
    #[error("command exited {code:?}: {stderr}")]
    NonZeroExit { code: Option<i32>, stderr: String },
    #[error("gh exited {code:?}: {stderr}")]
    GhFailed { code: Option<i32>, stderr: String },
    #[error("gh --json output is not valid JSON: {snippet}")]
    GhBadJson { snippet: String },
    #[error("could not spawn `{program}`: {msg}")]
    Spawn { program: String, msg: String },
    #[error("bad pattern: {msg}")]
    BadPattern { msg: String },
    #[error("{op} failed: {msg}")]
    Io { op: &'static str, msg: String },
    #[error("tool stream ended without a terminal event")]
    MissingTerminal,
    #[error("tool stream produced a second terminal event")]
    DoubleTerminal,
    #[error("tool stream produced progress after the terminal event")]
    TrailingProgress,
}
