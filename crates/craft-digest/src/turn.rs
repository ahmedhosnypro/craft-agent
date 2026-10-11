//! The bounded digest model turn: the digest tool posture, the fail-closed
//! outcome mapping, and the seam that lands the final assistant text in
//! the bundle.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use craft_core::provider::Provider;
use craft_core::types::Usage;
use craft_runtime::{CancelCategory, ExitStatus, RunConfig, Runner, RuntimeEvent};
use craft_tools::gh::GhTool;
use craft_tools::glob::GlobTool;
use craft_tools::grep::GrepTool;
use craft_tools::read::ReadFileTool;
use craft_tools::{Registry, ToolLimits};

use crate::bundle::Bundle;
use crate::error::{DigestError, bounded};

/// The digest posture: read, glob, grep, and gh — no bash, no repo writes.
/// Unregistered names round-trip as error results, never as denials.
pub fn digest_registry() -> Registry {
    let limits = ToolLimits::default();
    let mut registry = Registry::new(limits);
    registry.register(Box::new(ReadFileTool::new(limits)));
    registry.register(Box::new(GlobTool::new(limits)));
    registry.register(Box::new(GrepTool::new(limits)));
    registry.register(Box::new(GhTool::new(limits)));
    registry
}

/// The turn's motion bounds; the caller owns defaults and CLI overrides.
pub struct TurnConfig {
    pub workspace_root: PathBuf,
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub idle_timeout: Option<Duration>,
}

/// What a completed turn reports back to the pipeline.
pub struct TurnReport {
    pub turns: u32,
    pub usage: Usage,
}

/// Drive one bounded run over the digest prompt. A non-zero exit is a typed
/// error carrying the cancellation category — the digest fails closed and
/// the bundle excerpt is never written from a failed turn.
pub async fn run_turn<P: Provider + Send + Sync>(
    provider: Arc<P>,
    config: &TurnConfig,
    prompt: &str,
    bundle: &mut Bundle,
) -> Result<TurnReport, DigestError> {
    let registry = digest_registry();
    let mut run_config = RunConfig::new(config.workspace_root.clone());
    run_config.max_turns = config.max_turns;
    run_config.max_total_tokens = config.max_total_tokens;
    run_config.idle_timeout = config.idle_timeout;
    let runner = Runner::new(provider, registry, run_config);
    let mut final_text = String::new();
    let outcome = runner
        .run(prompt, |event| match event {
            RuntimeEvent::TurnStarted { .. } => final_text.clear(),
            RuntimeEvent::AssistantDelta { text, .. } => final_text.push_str(&text),
            _ => {}
        })
        .await;
    if outcome.exit != ExitStatus::Ok {
        return Err(DigestError::Turn {
            category: category_name(outcome.category).to_string(),
            detail: bounded(&outcome.detail),
        });
    }
    let report = TurnReport {
        turns: outcome.turns,
        usage: outcome.usage,
    };
    let repos: Vec<String> = bundle.repos.iter().map(|repo| repo.repo.clone()).collect();
    for repo in &repos {
        bundle.set_agents_excerpt(repo, &final_text);
    }
    Ok(report)
}

/// The category names of the stream-json contract.
fn category_name(category: CancelCategory) -> &'static str {
    match category {
        CancelCategory::Completed => "completed",
        CancelCategory::Error => "error",
        CancelCategory::MaxTurns => "max_turns",
        CancelCategory::Stationarity => "stationarity",
        CancelCategory::Budget => "budget",
        CancelCategory::SensitivePath => "sensitive_path",
        CancelCategory::EscapesWorkspace => "escapes_workspace",
    }
}
