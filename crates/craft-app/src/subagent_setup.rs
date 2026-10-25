//! Subagent run wiring: the three cap knobs from the environment, the
//! agent catalog, the shared hub over the mounted backend, and the
//! root wiring the parent registry and run config share. A depth cap
//! of 0 disables the spawn pair entirely; hostile knob values fail
//! the run setup with one actionable line.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use craft_core::agents::{AgentFile, load_catalog};
use craft_providers::backend::Backend;
use craft_runtime::{RunConfig, SubagentCaps, SubagentHub, SubagentSettings, SubagentWiring};
use craft_tools::ToolLimits;

use crate::agent_select::system_prompt;
use crate::cli::RunCli;
use crate::error::AppError;
use crate::prompt::DEFAULT_SYSTEM_PROMPT;

/// Mounts the subagent hub for a run: caps resolve from
/// `CRAFT_AGENT_SUBAGENT_MAX_DEPTH` / `_MAX_TOTAL` /
/// `_MAX_CONCURRENCY` (defaults 2 / 32 / 4), the profile catalog
/// loads from the workspace, and children inherit the run's budget
/// shape, per-request cap, idle timeout, and session stem.
pub fn mount(
    config: &RunConfig,
    workspace_root: &Path,
    provider: Arc<Backend>,
) -> Result<Option<SubagentWiring>, AppError> {
    let caps = SubagentCaps::from_env().map_err(|detail| AppError::SubagentCap { detail })?;
    if caps.max_depth == 0 {
        return Ok(None);
    }
    let catalog =
        load_catalog(workspace_root).map_err(|source| AppError::AgentCatalog { source })?;
    let settings = SubagentSettings {
        caps,
        run_token_cap: config.max_total_tokens,
        per_request_tokens: config.max_tokens,
        default_system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
        catalog: Arc::new(catalog),
        limits: ToolLimits::default(),
        workspace_root: workspace_root.to_path_buf(),
        parent_session: config.session_path.clone(),
        idle_timeout: config.idle_timeout,
        step_attempts: config.step_attempts,
        step_patience: None,
    };
    let hub = SubagentHub::mounting(settings, provider);
    Ok(Some(SubagentWiring::root(hub)))
}

pub fn build_run_config(
    run: &RunCli,
    agent: Option<&AgentFile>,
    workspace_root: PathBuf,
    session_path: PathBuf,
) -> RunConfig {
    let mut config = RunConfig::new(workspace_root);
    config.system = Some(system_prompt(agent));
    config.session_path = Some(session_path);
    if let Some(max_turns) = run.max_turns.or(agent.and_then(|agent| agent.max_turns)) {
        config.max_turns = max_turns;
    }
    if let Some(max_tokens) = run.max_tokens {
        config.max_total_tokens = Some(max_tokens);
    }
    if let Some(secs) = run.idle_timeout_secs {
        config.idle_timeout = Some(Duration::from_secs(secs));
    }
    config
}

pub fn default_session_path(workspace_root: &Path) -> Result<PathBuf, AppError> {
    let dir = workspace_root.join(".craft-agent").join("sessions");
    std::fs::create_dir_all(&dir).map_err(|source| AppError::SessionDir {
        path: dir.clone(),
        source,
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or_else(|_| 0, |elapsed| elapsed.as_nanos());
    Ok(dir.join(format!("{stamp}.jsonl")))
}
