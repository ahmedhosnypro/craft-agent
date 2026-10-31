//! `digest`: validate inputs, then hand off to the digest pipeline.

use std::path::Path;
use std::time::Duration;

use craft_digest::clone_runner::CloneRunner;
use craft_digest::collect::{GhRunner, parse_repo_list};
use craft_digest::jscpd_runner::JscpdRunner;
use craft_digest::pipeline::{DigestDeps, DigestOptions, run_digest};

use crate::cli::DigestCli;
use crate::config::{EnvConfig, env_from_process, mount_backend, resolve_env_config};
use crate::error::AppError;
use crate::redaction;
use crate::summary;

/// Curated repo list shipped with the workspace config.
pub const CURATED_REPOS_RELATIVE_PATH: &str = "config/digest-repos.txt";

/// The resolved launch environment: the process map, the config it
/// produced, and the redactor both share.
struct DigestEnv<'a> {
    env: &'a crate::config::EnvMap,
    config: &'a EnvConfig,
    redactor: &'a std::sync::Arc<craft_redact::Redactor>,
}

/// Resolves the repo set: explicit `--repos` wins over the curated list.
pub fn resolve_repos(
    explicit: Option<Vec<String>>,
    curated_path: &Path,
) -> Result<Vec<String>, AppError> {
    match explicit {
        Some(list) => {
            let repos: Vec<String> = list
                .iter()
                .map(|repo| repo.trim().to_string())
                .filter(|repo| !repo.is_empty())
                .collect();
            if repos.is_empty() {
                return Err(AppError::EmptyReposFlag);
            }
            Ok(repos)
        }
        None => load_curated_repos(curated_path),
    }
}

pub fn load_curated_repos(path: &Path) -> Result<Vec<String>, AppError> {
    let raw = std::fs::read_to_string(path).map_err(|source| AppError::CuratedRepos {
        path: path.to_path_buf(),
        source,
    })?;
    let repos = parse_repo_list(&raw);
    if repos.is_empty() {
        return Err(AppError::EmptyRepoList {
            path: path.to_path_buf(),
        });
    }
    Ok(repos)
}

pub fn execute_digest(digest: DigestCli) -> Result<i32, AppError> {
    let org = digest.org.trim();
    if org.is_empty() {
        return Err(AppError::EmptyOrg);
    }
    let workspace_root =
        std::env::current_dir().map_err(|source| AppError::WorkspaceRoot { source })?;
    let curated = workspace_root.join(CURATED_REPOS_RELATIVE_PATH);
    let repos = resolve_repos(digest.repos.clone(), &curated)?;
    let env = env_from_process();
    let redactor = std::sync::Arc::new(craft_redact::Redactor::from_env());
    redaction::emit_add_mask(&redactor, &env);
    let config = resolve_env_config(&env)?;
    let resolved = DigestEnv {
        env: &env,
        config: &config,
        redactor: &redactor,
    };
    dispatch_digest(&digest, org, &repos, &resolved)
}

/// The single seam to the craft-digest pipeline: mount the resolved
/// backend, drive the mission, and report the digest path (or the
/// dry-run output) through the exit code.
fn dispatch_digest(
    digest: &DigestCli,
    org: &str,
    repos: &[String],
    resolved: &DigestEnv<'_>,
) -> Result<i32, AppError> {
    let backend = mount_backend(resolved.config)?;
    let (options, caps) = digest_options(digest, org, repos)?;
    let deps = DigestDeps {
        gh: GhRunner::new(),
        clone_runner: CloneRunner::new(),
        jscpd: JscpdRunner::new(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| AppError::AsyncRuntime { source })?;
    let outcome = runtime
        .block_on(run_digest(backend, &deps, &options, &mut std::io::stdout()))
        .map_err(|source| AppError::Digest { source })?;
    report_digest(&outcome, &caps, resolved);
    Ok(0)
}

/// The two caps the summary table reports next to the run counts.
struct DigestCaps {
    max_turns: u32,
    max_tokens: u64,
}

fn digest_options(
    digest: &DigestCli,
    org: &str,
    repos: &[String],
) -> Result<(DigestOptions, DigestCaps), AppError> {
    let workspace_root =
        std::env::current_dir().map_err(|source| AppError::WorkspaceRoot { source })?;
    let caps = DigestCaps {
        max_turns: digest.max_turns.unwrap_or(40),
        max_tokens: digest.max_tokens.unwrap_or(4_000_000),
    };
    let env = env_from_process();
    let context = craft_runtime::ContextKnobs::resolve(
        &crate::subagent_setup::cli_knobs(
            digest.max_context_tokens,
            digest.max_input_tokens,
            digest.max_output_tokens,
            digest.thinking_level.clone(),
        ),
        &craft_runtime::ContextKnobs::default(),
        &crate::subagent_setup::env_knobs(&env)?,
    );
    context
        .validate()
        .map_err(|detail| AppError::ContextKnob { detail })?;
    let options = DigestOptions {
        org: org.to_string(),
        repos: repos.to_vec(),
        lookback: digest.since,
        dry_run: digest.dry_run,
        workspace_root,
        prompt_path: None,
        max_turns: caps.max_turns,
        max_total_tokens: caps.max_tokens,
        idle_timeout: digest.idle_timeout_secs.map(Duration::from_secs),
        context,
    };
    Ok((options, caps))
}

/// Print the digest path and append the mission summary.
fn report_digest(
    outcome: &craft_digest::pipeline::DigestOutcome,
    caps: &DigestCaps,
    resolved: &DigestEnv<'_>,
) {
    if let Some(path) = &outcome.digest_path {
        println!("digest: {}", path.display());
    }
    summary::emit_digest_summary(
        resolved.env,
        outcome.digest_path.as_deref(),
        outcome.turns,
        summary::SummaryFacts {
            mission: "digest",
            model: resolved.config.model.clone(),
            provider: summary::provider_name(resolved.config.provider).to_string(),
            turn_cap: Some(caps.max_turns),
            token_budget: Some(caps.max_tokens),
            rows: redaction::summary_rows(resolved.redactor),
        },
    );
}
