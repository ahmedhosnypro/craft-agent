//! The workflow run's subagent hub: mounted once per run over the
//! default model's backend and shared by every step.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use craft_core::agents::load_catalog;
use craft_runtime::{
    ContextKnobs, DEFAULT_REQUEST_TOKENS, SubagentCaps, SubagentHub, SubagentSettings,
    SubagentWiring,
};
use craft_tools::ToolLimits;

use crate::config::EnvConfig;
use crate::error::AppError;
use crate::prompt::DEFAULT_SYSTEM_PROMPT;

use super::production_mount;

/// Mounts the hub: a fan-out step's registry carries the spawn pair
/// while the run's caps, catalog, and token pool bind the whole tree.
/// A mount failure aborts the run — a step must never silently degrade
/// to serial work because its hub was missing.
pub(crate) fn mount_workflow_hub(
    env_config: &EnvConfig,
    workspace_root: &Path,
    run_token_cap: Option<u64>,
    idle_timeout: Option<Duration>,
    sessions_dir: &Path,
    context: ContextKnobs,
) -> Result<SubagentWiring, AppError> {
    let caps = SubagentCaps::from_env().map_err(|detail| AppError::SubagentCap { detail })?;
    if caps.max_depth == 0 {
        let detail = String::from(
            "CRAFT_AGENT_SUBAGENT_MAX_DEPTH is 0 — workflow steps could not spawn subagents",
        );
        return Err(AppError::SubagentCap { detail });
    }
    let catalog =
        load_catalog(workspace_root).map_err(|source| AppError::WorkflowCatalog { source })?;
    let settings = SubagentSettings {
        caps,
        run_token_cap,
        per_request_tokens: DEFAULT_REQUEST_TOKENS,
        context,
        default_system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
        catalog: Arc::new(catalog),
        limits: ToolLimits::default(),
        workspace_root: workspace_root.to_path_buf(),
        parent_session: Some(sessions_dir.join("subagents.jsonl")),
        idle_timeout,
        step_attempts: None,
        step_patience: None,
    };
    let provider = production_mount(env_config.clone())(&env_config.model)
        .map_err(|detail| AppError::SubagentCap { detail })?;
    Ok(SubagentWiring::root(SubagentHub::mounting(
        settings, provider,
    )))
}
