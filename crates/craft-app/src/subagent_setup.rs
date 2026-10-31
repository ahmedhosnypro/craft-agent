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
use craft_runtime::{
    ContextKnobs, RunConfig, SubagentCaps, SubagentHub, SubagentSettings, SubagentWiring,
};
use craft_tools::ToolLimits;

use crate::agent_select::system_prompt;
use crate::cli::RunCli;
use crate::config::EnvMap;
use crate::error::AppError;
use crate::prompt::DEFAULT_SYSTEM_PROMPT;

/// The four context knobs a CLI struct carries.
pub fn cli_knobs(
    max_context_tokens: Option<u64>,
    max_input_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    thinking_level: Option<String>,
) -> ContextKnobs {
    ContextKnobs {
        max_context_tokens,
        max_input_tokens,
        max_output_tokens,
        thinking_level,
    }
}

/// The env knobs from the process map; a hostile value is a typed error
/// naming its knob.
pub fn env_knobs(env: &EnvMap) -> Result<ContextKnobs, AppError> {
    ContextKnobs::from_env(&|name| env.get(name).cloned())
        .map_err(|detail| AppError::ContextKnob { detail })
}

/// Mounts the subagent hub for a run: caps resolve from
/// `CRAFT_AGENT_SUBAGENT_MAX_DEPTH` / `_MAX_TOTAL` /
/// `_MAX_CONCURRENCY` (defaults 2 / 32 / 4), the profile catalog
/// loads from the workspace, and children inherit the run's budget
/// shape, per-request cap, context knobs, idle timeout, and session
/// stem.
pub fn mount(
    config: &RunConfig,
    context: ContextKnobs,
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
        context,
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

/// Builds the run config: CLI flags beat the agent profile's
/// frontmatter, which beats the env knobs; hostile env values fail the
/// run setup with one actionable line naming the knob.
pub fn build_run_config(
    run: &RunCli,
    agent: Option<&AgentFile>,
    env: &EnvMap,
    workspace_root: PathBuf,
    session_path: PathBuf,
) -> Result<RunConfig, AppError> {
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
    let knobs = ContextKnobs::resolve(
        &cli_knobs(
            run.max_context_tokens,
            run.max_input_tokens,
            run.max_output_tokens,
            run.thinking_level.clone(),
        ),
        &ContextKnobs::from_agent(agent),
        &env_knobs(env)?,
    );
    knobs
        .apply(&mut config)
        .map_err(|detail| AppError::ContextKnob { detail })?;
    Ok(config)
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
