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

/// Curated repo list shipped with the workspace config.
pub const CURATED_REPOS_RELATIVE_PATH: &str = "config/digest-repos.txt";

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
    let config = resolve_env_config(&env)?;
    dispatch_digest(&digest, org, &repos, &config)
}

/// The single seam to the craft-digest pipeline: mount the resolved
/// backend, drive the mission, and report the digest path (or the
/// dry-run output) through the exit code.
fn dispatch_digest(
    digest: &DigestCli,
    org: &str,
    repos: &[String],
    config: &EnvConfig,
) -> Result<i32, AppError> {
    let backend = mount_backend(config)?;
    let workspace_root =
        std::env::current_dir().map_err(|source| AppError::WorkspaceRoot { source })?;
    let options = DigestOptions {
        org: org.to_string(),
        repos: repos.to_vec(),
        lookback: digest.since,
        dry_run: digest.dry_run,
        workspace_root,
        prompt_path: None,
        max_turns: digest.max_turns.unwrap_or(40),
        max_total_tokens: digest.max_tokens.unwrap_or(4_000_000),
        idle_timeout: digest.idle_timeout_secs.map(Duration::from_secs),
    };
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
    if let Some(path) = outcome.digest_path {
        println!("digest: {}", path.display());
    }
    Ok(0)
}
