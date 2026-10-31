//! The workflow run's context knobs: CLI flags over the step
//! profiles' frontmatter over the environment.

use craft_runtime::ContextKnobs;

use crate::config::EnvMap;
use crate::error::AppError;
use crate::subagent_setup;

/// Resolves the context knobs for a workflow run and validates them.
/// CLI flags beat each step profile's frontmatter, which beats the
/// env knobs; a hostile env value fails the command with the knob
/// named.
pub(crate) fn resolve_context(
    max_context_tokens: Option<u64>,
    max_input_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    thinking_level: Option<String>,
    env: &EnvMap,
) -> Result<ContextKnobs, AppError> {
    let knobs = ContextKnobs::resolve(
        &subagent_setup::cli_knobs(
            max_context_tokens,
            max_input_tokens,
            max_output_tokens,
            thinking_level,
        ),
        &ContextKnobs::default(),
        &subagent_setup::env_knobs(env)?,
    );
    knobs
        .validate()
        .map_err(|detail| AppError::ContextKnob { detail })?;
    Ok(knobs)
}
