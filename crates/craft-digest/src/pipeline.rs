//! The digest pipeline: window -> collect -> clones -> jscpd -> the
//! bounded model turn -> render -> scrub -> `out/digest.md` (or the
//! dry-run sink). The assembly the CLI's digest command drives.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use craft_core::provider::Provider;

use crate::bundle::Bundle;
use crate::clone_runner::{CloneCtx, CloneRunner, clone_all};
use crate::collect::{GhRunner, collect_all};
use crate::dry_run::emit_dry_run;
use crate::error::{DigestError, bounded};
use crate::jscpd_runner::{JscpdRunner, run_jscpd};
use crate::prompt::{build_digest_prompt, default_prompt_path, load_prompt};
use crate::render::render_digest;
use crate::scrub::scrub;
use crate::turn::{TurnConfig, TurnReport, run_turn};
use crate::window::{Window, resolve_window_lookback};

/// The action contract's digest location, relative to the workspace root.
pub const DIGEST_OUTPUT_RELATIVE_PATH: &str = "out/digest.md";

/// The injectable collectors and runners the pipeline drives.
pub struct DigestDeps {
    pub gh: GhRunner,
    pub clone_runner: CloneRunner,
    pub jscpd: JscpdRunner,
}

/// One digest run's inputs; the caller owns CLI defaults.
pub struct DigestOptions {
    pub org: String,
    pub repos: Vec<String>,
    pub lookback: Duration,
    pub dry_run: bool,
    pub workspace_root: PathBuf,
    /// Overrides the workspace's `config/digest-prompt.md` when set.
    pub prompt_path: Option<PathBuf>,
    pub max_turns: u32,
    pub max_total_tokens: u64,
    pub idle_timeout: Option<Duration>,
    /// Context knobs resolved at the CLI/env layer; folded into the
    /// turn's run config.
    pub context: craft_runtime::ContextKnobs,
}

/// The terminal record: the written digest (None on dry-run), the window,
/// the collected bundle, and the turns the model took.
#[derive(Debug)]
pub struct DigestOutcome {
    pub digest_path: Option<PathBuf>,
    pub window: Window,
    pub bundle: Bundle,
    pub turns: u32,
}

/// Run the whole digest mission. Per-repo collector failures stay error
/// rows inside the bundle; the typed Err paths are the ones that make a
/// digest impossible or untrustworthy (window, prompt, turn, output).
pub async fn run_digest<P: Provider + Send + Sync>(
    provider: Arc<P>,
    deps: &DigestDeps,
    options: &DigestOptions,
    out: &mut dyn Write,
) -> Result<DigestOutcome, DigestError> {
    let window = resolve_window_lookback(options.lookback)?;
    let mut bundle = collect_all(&deps.gh, &options.org, &options.repos, &window);
    // Pid keeps the scratch dir unique even when the wall clock is unavailable.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let scratch = options
        .workspace_root
        .join(".craft-agent")
        .join("digest-clones")
        .join(format!("{nanos}-{}", std::process::id()));
    let ctx = AssemblyCtx {
        deps,
        options,
        scratch: &scratch,
    };
    let assembled = assemble(provider, &ctx, &mut bundle, out).await;
    cleanup_scratch(&options.workspace_root, &scratch);
    let (digest_path, turns) = assembled?;
    Ok(DigestOutcome {
        digest_path,
        window,
        bundle,
        turns,
    })
}

/// The per-run inputs every assembly stage shares.
struct AssemblyCtx<'a> {
    deps: &'a DigestDeps,
    options: &'a DigestOptions,
    scratch: &'a Path,
}

async fn assemble<P: Provider + Send + Sync>(
    provider: Arc<P>,
    ctx: &AssemblyCtx<'_>,
    bundle: &mut Bundle,
    out: &mut dyn Write,
) -> Result<(Option<PathBuf>, u32), DigestError> {
    clone_all(
        &CloneCtx {
            runner: &ctx.deps.clone_runner,
            org: &ctx.options.org,
            repos: &ctx.options.repos,
            scratch_root: ctx.scratch,
        },
        bundle,
    )?;
    let report_dir = ctx.scratch.join("jscpd-report");
    bundle.duplicates = run_jscpd(&ctx.deps.jscpd, ctx.scratch, &report_dir, bundle)?;
    let prompt_path = ctx
        .options
        .prompt_path
        .clone()
        .unwrap_or_else(|| default_prompt_path(&ctx.options.workspace_root));
    let template = load_prompt(&prompt_path)?;
    let prompt = build_digest_prompt(&template, bundle, ctx.scratch);
    let turn_config = TurnConfig {
        workspace_root: ctx.options.workspace_root.clone(),
        max_turns: ctx.options.max_turns,
        max_total_tokens: Some(ctx.options.max_total_tokens),
        idle_timeout: ctx.options.idle_timeout,
        context: ctx.options.context.clone(),
    };
    let report: TurnReport = run_turn(provider, &turn_config, &prompt, bundle).await?;
    let digest_path = if ctx.options.dry_run {
        emit_dry_run(out, bundle)?;
        None
    } else {
        Some(write_digest(
            &ctx.options.workspace_root,
            &scrub(&render_digest(bundle)),
        )?)
    };
    Ok((digest_path, report.turns))
}

/// Write the scrubbed digest; `out/` is created on demand.
fn write_digest(workspace_root: &Path, digest: &str) -> Result<PathBuf, DigestError> {
    let path = workspace_root.join(DIGEST_OUTPUT_RELATIVE_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| DigestError::Io(bounded(&err.to_string())))?;
    }
    fs::write(&path, digest).map_err(|err| DigestError::Io(bounded(&err.to_string())))?;
    Ok(path)
}

/// Best-effort scratch removal: the run dir first, then the parents, which
/// only go away when this run was their only content. The walk stops at
/// the workspace root, which the run does not own.
fn cleanup_scratch(workspace_root: &Path, scratch: &Path) {
    let _ = fs::remove_dir_all(scratch);
    let mut parent = scratch.parent();
    while let Some(dir) = parent {
        if dir == workspace_root || fs::remove_dir(dir).is_err() {
            break;
        }
        parent = dir.parent();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_dir(label: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp/craft-digest-corpus")
            .join(format!("cleanup-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cleanup_stops_at_the_workspace_root() {
        let workspace = fresh_dir("ws");
        let scratch = workspace
            .join(".craft-agent")
            .join("digest-clones")
            .join("1");
        fs::create_dir_all(&scratch).unwrap();
        cleanup_scratch(&workspace, &scratch);
        assert!(workspace.exists(), "cleanup removed the workspace root");
        assert!(!workspace.join(".craft-agent").exists());
        let _ = fs::remove_dir_all(&workspace);
    }
}
