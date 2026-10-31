//! Agent profile files: the `.craft-agent/agents/*.md` format — flat
//! YAML-subset frontmatter plus a markdown body that becomes the
//! selected profile's system prompt. Field rules mirror the agent-file
//! gate; discovery is fail-soft, selection fail-closed.

pub mod globs;
pub mod load;
pub mod parse;
pub(crate) mod tools;
pub mod validate;

pub use load::{discover_dir, discover_file, load_catalog, load_dir, load_file};

use std::path::{Path, PathBuf};

/// The builtin tool ids an agent file may name in `tools` or
/// `disallowedTools`.
pub const TOOL_IDS: [&str; 10] = [
    "bash",
    "read_file",
    "write_file",
    "edit_file",
    "glob",
    "grep",
    "gh",
    "spawn_agent",
    "spawn_agents",
    "poll_agents",
];

/// Frontmatter keys the format accepts; anything else is an error.
pub(crate) const KNOWN_KEYS: [&str; 16] = [
    "name",
    "description",
    "model",
    "promptMode",
    "tools",
    "disallowedTools",
    "denyRead",
    "denyWrite",
    "maxTurns",
    "timeoutSecs",
    "background",
    "agentsMd",
    "maxContextTokens",
    "maxInputTokens",
    "maxOutputTokens",
    "thinking",
];

/// How the body combines with the default system prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptMode {
    /// The default prompt, then the body.
    #[default]
    Extend,
    /// The body is the entire system prompt.
    Full,
}

/// One parsed, validated agent profile file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFile {
    pub path: PathBuf,
    pub name: String,
    pub description: String,
    /// `inherit` and an absent field both normalize to None.
    pub model: Option<String>,
    pub prompt_mode: PromptMode,
    /// None inherits every registered tool; Some is the allowlist.
    pub tools: Option<Vec<String>>,
    /// Applied after the allowlist or the inherited set.
    pub disallowed_tools: Vec<String>,
    /// Path globs this profile may never read.
    pub deny_read: Vec<String>,
    /// Path globs this profile may never write.
    pub deny_write: Vec<String>,
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
    /// Context window cap in tokens; enables auto-compaction when set.
    pub max_context_tokens: Option<u64>,
    /// Input-side compaction trigger limit; None falls back to the
    /// context cap.
    pub max_input_tokens: Option<u64>,
    /// Per-request output cap in tokens.
    pub max_output_tokens: Option<u64>,
    /// Thinking level string; `none`/`off` disables thinking, anything
    /// else passes through to the provider verbatim.
    pub thinking: Option<String>,
    pub background: bool,
    pub agents_md: bool,
    /// The prompt body, edge whitespace trimmed.
    pub body: String,
}

/// Fail-closed validation error naming the file, line, and gate rule
/// that fired. Rule ids mirror the agent-file gate; `FIELD01` and `Io`
/// are runtime-only rules the gate does not lint.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentFileError {
    #[error("{path}:{line}: FM01 frontmatter: {message}")]
    Frontmatter {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: FM02 required-field: {message}")]
    RequiredField {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: FM03 name-charset: {message}")]
    NameCharset {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: FM04 name-stem: {message}")]
    NameStem {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: FM05 description: {message}")]
    Description {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: FM06 description-trigger: {message}")]
    Trigger {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: FIELD01 value-shape: {message}")]
    ValueShape {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: MODEL01 model: {message}")]
    Model {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: TOOLS01 tools: {message}")]
    Tools {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: MD01 body: {message}")]
    Body {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}:{line}: MD02 body-length: {message}")]
    BodyLength {
        path: PathBuf,
        line: u32,
        message: String,
    },
    #[error("{path}: cannot read: {message}")]
    Io { path: PathBuf, message: String },
}

/// An invalid file kept for selection errors, keyed by its parsed name
/// when the frontmatter yielded one, with the file stem as a second
/// spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub key: String,
    pub stem: String,
    pub error: AgentFileError,
}

/// One discovery pass: the valid profiles plus the files skipped with
/// their errors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentCatalog {
    pub agents: Vec<AgentFile>,
    pub skipped: Vec<SkippedFile>,
}

/// What a name resolves to in a catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogHit<'a> {
    Found(&'a AgentFile),
    Invalid(&'a AgentFileError),
    Missing,
}

impl AgentCatalog {
    /// Valid profiles match by name first; a skipped file whose parsed
    /// name or file stem matches surfaces its validation error
    /// instead.
    pub fn lookup(&self, name: &str) -> CatalogHit<'_> {
        if let Some(agent) = self.agents.iter().find(|agent| agent.name == name) {
            return CatalogHit::Found(agent);
        }
        if let Some(skip) = self
            .skipped
            .iter()
            .find(|skip| skip.key == name || skip.stem == name)
        {
            return CatalogHit::Invalid(&skip.error);
        }
        CatalogHit::Missing
    }

    /// Sorted selectable names, valid profiles only.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();
        names.sort_unstable();
        names
    }
}

/// The file stem (name minus one trailing `.md`).
pub(crate) fn file_stem(path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    match name.strip_suffix(".md") {
        Some(stem) => stem.to_string(),
        None => name,
    }
}
