//! `--agent` selection: resolve a profile from the closest
//! `.craft-agent/agents` (or one explicit file), then apply it — the
//! body becomes the system prompt, the registry keeps the allowlist
//! minus the denylist, and the profile's `timeoutSecs` becomes a
//! wall-clock cap whose expiry still ends the run with one terminal
//! record.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use craft_core::agents::{AgentFile, AgentFileError, CatalogHit, load_catalog, load_file};
use craft_core::types::Usage;
use craft_runtime::{CancelCategory, ExitStatus, RunOutcome, RuntimeEvent};
use craft_tools::bash::BashTool;
use craft_tools::edit::EditFileTool;
use craft_tools::gh::GhTool;
use craft_tools::glob::GlobTool;
use craft_tools::grep::GrepTool;
use craft_tools::read::ReadFileTool;
use craft_tools::registry::Registry;
use craft_tools::tool::{Tool, ToolLimits};
use craft_tools::write::WriteFileTool;

use crate::cli::RunCli;
use crate::run_cmd::DEFAULT_SYSTEM_PROMPT;

/// Selection failures: one actionable line each; a selected invalid
/// file fails the run, unselected invalid files were only noted.
#[derive(Debug, thiserror::Error)]
pub enum AgentSelectError {
    #[error("agent file is invalid: {source}")]
    File { source: AgentFileError },
    #[error("unknown agent profile `{name}`; available: {available}")]
    Unknown { name: String, available: String },
}

/// Resolves the selected profile: `--agent-file` loads that one file
/// fail-closed, `--agent` looks the name up in the closest agents
/// catalog, and neither flag selects nothing.
pub fn resolve(run: &RunCli, workspace_root: &Path) -> Result<Option<AgentFile>, AgentSelectError> {
    if let Some(path) = &run.agent_file {
        return load_file(path)
            .map(Some)
            .map_err(|source| AgentSelectError::File { source });
    }
    let Some(name) = &run.agent else {
        return Ok(None);
    };
    let catalog =
        load_catalog(workspace_root).map_err(|source| AgentSelectError::File { source })?;
    for skip in &catalog.skipped {
        eprintln!(
            "craft-agent: skipped agent file `{}`: {}",
            skip.key, skip.error
        );
    }
    match catalog.lookup(name) {
        CatalogHit::Found(agent) => Ok(Some(agent.clone())),
        CatalogHit::Invalid(error) => Err(AgentSelectError::File {
            source: error.clone(),
        }),
        CatalogHit::Missing => Err(AgentSelectError::Unknown {
            name: name.clone(),
            available: listing(&catalog),
        }),
    }
}

fn listing(catalog: &craft_core::agents::AgentCatalog) -> String {
    let names = catalog.names();
    if names.is_empty() {
        String::from("(none found)")
    } else {
        names.join(", ")
    }
}

/// The effective system prompt: the default prompt, the body alone
/// (`promptMode: full`), or the default followed by the body
/// (`promptMode: extend`).
pub fn system_prompt(agent: Option<&AgentFile>) -> String {
    match agent {
        None => DEFAULT_SYSTEM_PROMPT.to_string(),
        Some(agent) => match agent.prompt_mode {
            craft_core::agents::PromptMode::Full => agent.body.clone(),
            craft_core::agents::PromptMode::Extend => {
                format!("{DEFAULT_SYSTEM_PROMPT}\n\n{}", agent.body)
            }
        },
    }
}

/// The registry for the selected profile: an absent `tools` field
/// inherits every builtin, an allowlist keeps only its members, and
/// `disallowedTools` removes ids from whatever remains.
pub fn registry(agent: Option<&AgentFile>) -> Registry {
    let limits = ToolLimits::default();
    let Some(agent) = agent else {
        return Registry::with_builtins(limits);
    };
    let allow = agent.tools.as_deref();
    let deny = agent.disallowed_tools.as_slice();
    if allow.is_none() && deny.is_empty() {
        return Registry::with_builtins(limits);
    }
    let mut registry = Registry::new(limits);
    for id in craft_core::agents::TOOL_IDS {
        let allowed = allow.is_none_or(|list| list.iter().any(|tool| tool == id));
        let denied = deny.iter().any(|tool| tool == id);
        if allowed && !denied {
            register(&mut registry, id, limits);
        }
    }
    registry
}

fn register(registry: &mut Registry, id: &str, limits: ToolLimits) {
    let tool: Box<dyn Tool> = match id {
        "bash" => Box::new(BashTool::new(limits)),
        "read_file" => Box::new(ReadFileTool::new(limits)),
        "write_file" => Box::new(WriteFileTool::new(limits)),
        "edit_file" => Box::new(EditFileTool::new(limits)),
        "glob" => Box::new(GlobTool::new(limits)),
        "grep" => Box::new(GrepTool::new(limits)),
        "gh" => Box::new(GhTool::new(limits)),
        _ => return,
    };
    registry.register(tool);
}

/// What the event stream already reported, so a wall-clock kill can
/// still end the run with an honest terminal record.
#[derive(Debug)]
pub struct RunWatch {
    usage: Usage,
    turns_done: u32,
}

impl Default for RunWatch {
    fn default() -> Self {
        Self {
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                incomplete: false,
            },
            turns_done: 0,
        }
    }
}

impl RunWatch {
    /// Merges one turn's reported usage and counts the turn done.
    pub fn record(shared: &Mutex<Self>, event: &RuntimeEvent) {
        if let RuntimeEvent::TurnUsage { usage, .. } = event {
            let mut state = shared.lock().unwrap_or_else(|held| held.into_inner());
            state.usage = state.usage.merge(*usage);
            state.turns_done += 1;
        }
    }

    /// Ends the run at the wall-clock cap: exit 1, the reported usage
    /// marked incomplete; the caller emits the returned `run_finished`
    /// as the last event line.
    pub fn expire(
        shared: &Mutex<Self>,
        secs: u64,
        session_path: Option<&Path>,
    ) -> (RunOutcome, RuntimeEvent) {
        let state = shared.lock().unwrap_or_else(|held| held.into_inner());
        let mut usage = state.usage;
        usage.incomplete = true;
        let detail = format!("run exceeded the agent wall-clock cap ({secs}s)");
        let event = RuntimeEvent::RunFinished {
            exit_code: ExitStatus::Error.code(),
            category: CancelCategory::Error,
            detail: detail.clone(),
            usage,
            turns: state.turns_done,
        };
        let outcome = RunOutcome {
            exit: ExitStatus::Error,
            category: CancelCategory::Error,
            detail,
            usage,
            turns: state.turns_done,
            session_path: session_path.map(PathBuf::from),
        };
        (outcome, event)
    }
}
