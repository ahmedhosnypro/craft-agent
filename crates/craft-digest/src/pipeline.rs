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
use crate::clone_runner::{CloneRunner, clone_all};
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
    let scratch = options
        .workspace_root
        .join(".craft-agent")
        .join("digest-clones")
        .join(format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or_else(|_| 0, |elapsed| elapsed.as_nanos())
        ));
    let assembled = assemble(provider, deps, options, &mut bundle, &scratch, out).await;
    cleanup_scratch(&scratch);
    let (digest_path, turns) = assembled?;
    Ok(DigestOutcome {
        digest_path,
        window,
        bundle,
        turns,
    })
}

async fn assemble<P: Provider + Send + Sync>(
    provider: Arc<P>,
    deps: &DigestDeps,
    options: &DigestOptions,
    bundle: &mut Bundle,
    scratch: &Path,
    out: &mut dyn Write,
) -> Result<(Option<PathBuf>, u32), DigestError> {
    clone_all(
        &deps.clone_runner,
        &options.org,
        &options.repos,
        bundle,
        scratch,
    )?;
    let report_dir = scratch.join("jscpd-report");
    bundle.duplicates = run_jscpd(&deps.jscpd, scratch, &report_dir, bundle)?;
    let prompt_path = options
        .prompt_path
        .clone()
        .unwrap_or_else(|| default_prompt_path(&options.workspace_root));
    let template = load_prompt(&prompt_path)?;
    let prompt = build_digest_prompt(&template, bundle, scratch);
    let turn_config = TurnConfig {
        workspace_root: options.workspace_root.clone(),
        max_turns: options.max_turns,
        max_total_tokens: options.max_total_tokens,
        idle_timeout: options.idle_timeout,
    };
    let report: TurnReport = run_turn(provider, &turn_config, &prompt, bundle).await?;
    let digest_path = if options.dry_run {
        emit_dry_run(out, bundle)?;
        None
    } else {
        Some(write_digest(
            &options.workspace_root,
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
/// only go away when this run was their only content.
fn cleanup_scratch(scratch: &Path) {
    let _ = fs::remove_dir_all(scratch);
    let mut parent = scratch.parent();
    while let Some(dir) = parent {
        if fs::remove_dir(dir).is_err() {
            break;
        }
        parent = dir.parent();
    }
}
