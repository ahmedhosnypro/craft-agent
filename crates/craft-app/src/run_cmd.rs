//! `run`: mount a backend, drive the runner, stream events, exit.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use craft_runtime::{RunConfig, Runner, RuntimeEvent};
use craft_tools::Registry;

use crate::cli::{OutputFormat, PromptSource, RunCli};
use crate::config::{env_from_process, mount_backend, resolve_env_config};
use crate::error::AppError;
use crate::stream_json::{write_event, write_plain_event};

/// Prompt reads are bounded; a bigger prompt is rejected, not truncated.
pub const PROMPT_CAP: usize = 1024 * 1024;

pub const DEFAULT_SYSTEM_PROMPT: &str = "You are craft-agent, the storytold \
     suite's bring-your-own-key coding agent. Work inside the workspace \
     root, prefer minimal reviewable changes, and report results plainly.";

pub fn read_prompt(source: &PromptSource) -> Result<String, AppError> {
    let bytes = match source {
        PromptSource::Stdin => {
            let stdin = std::io::stdin();
            read_bounded(stdin.lock(), |source| AppError::PromptStdin { source })?
        }
        PromptSource::File(path) => {
            let file = std::fs::File::open(path).map_err(|source| AppError::PromptFile {
                path: path.clone(),
                source,
            })?;
            read_bounded(file, |source| AppError::PromptFile {
                path: path.clone(),
                source,
            })?
        }
    };
    String::from_utf8(bytes).map_err(|_| AppError::PromptNotUtf8)
}

fn read_bounded(
    mut reader: impl Read,
    on_io: impl Fn(std::io::Error) -> AppError,
) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk).map_err(&on_io)?;
        if n == 0 {
            return Ok(buf);
        }
        if buf.len() + n > PROMPT_CAP {
            return Err(AppError::PromptTooLarge { max: PROMPT_CAP });
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

pub fn build_run_config(run: &RunCli, workspace_root: PathBuf, session_path: PathBuf) -> RunConfig {
    let mut config = RunConfig::new(workspace_root);
    config.system = Some(DEFAULT_SYSTEM_PROMPT.to_string());
    config.session_path = Some(session_path);
    if let Some(max_turns) = run.max_turns {
        config.max_turns = max_turns;
    }
    if let Some(max_tokens) = run.max_tokens {
        config.max_total_tokens = max_tokens;
    }
    if let Some(secs) = run.idle_timeout_secs {
        config.idle_timeout = Some(Duration::from_secs(secs));
    }
    config
}

fn default_session_path(workspace_root: &Path) -> Result<PathBuf, AppError> {
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

pub fn execute_run(run: RunCli) -> Result<i32, AppError> {
    let prompt = read_prompt(&run.prompt)?;
    let env = env_from_process();
    let config = resolve_env_config(&env)?;
    let backend = mount_backend(&config)?;
    let workspace_root =
        std::env::current_dir().map_err(|source| AppError::WorkspaceRoot { source })?;
    let session_path = default_session_path(&workspace_root)?;
    let run_config = build_run_config(&run, workspace_root, session_path);
    let registry = Registry::with_builtins(craft_tools::ToolLimits::default());
    let runner = Runner::new(backend, registry, run_config);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| AppError::AsyncRuntime { source })?;
    let format = run.output_format;
    let outcome = runtime.block_on(runner.run(&prompt, |event| {
        emit(format, &event);
    }));
    Ok(outcome.exit_code())
}

fn emit(format: OutputFormat, event: &RuntimeEvent) {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let result = match format {
        OutputFormat::StreamJson => write_event(&mut out, event),
        OutputFormat::Plain => write_plain_event(&mut out, event),
    };
    if let Err(error) = result {
        eprintln!("craft-agent: run-event write failed: {error}");
    }
}
