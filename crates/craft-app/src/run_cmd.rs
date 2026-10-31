//! `run`: mount a backend, drive the runner, stream events, exit.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use craft_runtime::{Runner, RuntimeEvent, profile};
use craft_tools::ToolLimits;

use crate::agent_select::{RunWatch, resolve};
use crate::cli::RunCli;
use crate::config::{env_from_process, mount_backend, resolve_env_config};
use crate::error::AppError;
use crate::event_writer;
use crate::prompt::read_prompt;
use crate::redaction;
use crate::stream_json;
use crate::subagent_setup::{self, build_run_config, default_session_path};
use crate::summary;

/// Spawns the run-event writer; on failure the run still ends with its
/// terminal stream line, written straight to stdout.
fn spawn_run_writer(
    format: crate::cli::OutputFormat,
    redactor: &craft_redact::Redactor,
) -> Result<(event_writer::EventSink, event_writer::EventWriterHandle), AppError> {
    match event_writer::spawn_event_writer(format) {
        Ok(pair) => Ok(pair),
        Err(error) => {
            stream_json::write_terminal_line(
                std::io::stdout().lock(),
                format,
                redaction::terminal_detail(&error, redactor),
            );
            Err(error)
        }
    }
}

pub fn execute_run(run: RunCli) -> Result<i32, AppError> {
    let format = run.output_format;
    let redactor = Arc::new(craft_redact::Redactor::from_env());
    let env = env_from_process();
    redaction::emit_add_mask(&redactor, &env);
    execute_run_inner(run, &redactor).inspect_err(|error| {
        stream_json::write_terminal_line(
            std::io::stdout().lock(),
            format,
            redaction::terminal_detail(error, &redactor),
        );
    })
}

/// The two caps the summary table reports next to the run counts.
struct RunCaps {
    turn_cap: u32,
    token_budget: Option<u64>,
}

/// A mounted run ready to drive: the runner, the prompt it runs, the
/// wall-clock cap the profile names, and the session it writes to.
struct MountedRun {
    runner: Runner<craft_providers::backend::Backend>,
    prompt: String,
    caps: RunCaps,
    deadline: Option<u64>,
    /// The run's detached-pool wiring, drained when the run ends.
    pool: Option<craft_runtime::SubagentWiring>,
    format: crate::cli::OutputFormat,
    session_path: Option<std::path::PathBuf>,
    watch: Arc<Mutex<RunWatch>>,
}

/// Mounts the backend, the profile, and the subagent hub, returning the
/// run they describe. `None` names a profile that failed to resolve:
/// its own line is already printed.
fn mount_run(
    run: &RunCli,
    redactor: &Arc<craft_redact::Redactor>,
    config: &crate::config::EnvConfig,
    env: &crate::config::EnvMap,
) -> Result<Option<MountedRun>, AppError> {
    let prompt = read_prompt(&run.prompt)?;
    let backend = mount_backend(config)?;
    let workspace_root =
        std::env::current_dir().map_err(|source| AppError::WorkspaceRoot { source })?;
    let session_path = default_session_path(&workspace_root)?;
    let agent = match resolve(run, &workspace_root) {
        Ok(agent) => agent,
        Err(error) => {
            stream_json::write_terminal_line(
                std::io::stdout().lock(),
                run.output_format,
                redaction::terminal_detail(&error, redactor),
            );
            eprintln!("craft-agent: {error}");
            return Ok(None);
        }
    };
    let run_config = build_run_config(
        run,
        agent.as_ref(),
        env,
        workspace_root.clone(),
        session_path,
    )?;
    let caps = RunCaps {
        turn_cap: run_config.max_turns,
        token_budget: run_config.max_total_tokens,
    };
    let session_path = run_config.session_path.clone();
    let deadline = agent.as_ref().and_then(|agent| agent.timeout_secs);
    // Spawned children resolve CLI > their own profile frontmatter > env.
    let child_context = craft_runtime::ContextKnobs::resolve(
        &subagent_setup::cli_knobs(
            run.max_context_tokens,
            run.max_input_tokens,
            run.max_output_tokens,
            run.thinking_level.clone(),
        ),
        &craft_runtime::ContextKnobs::default(),
        &subagent_setup::env_knobs(env)?,
    );
    let subagents =
        subagent_setup::mount(&run_config, child_context, &workspace_root, backend.clone())?;
    let run_registry =
        profile::build_registry(agent.as_ref(), ToolLimits::default(), subagents.clone());
    let mut run_config = run_config;
    run_config.redactor = redactor.clone();
    run_config.subagents = subagents.clone();
    Ok(Some(MountedRun {
        runner: Runner::new(backend, run_registry, run_config),
        prompt,
        caps,
        deadline,
        pool: subagents,
        format: run.output_format,
        session_path,
        watch: Arc::new(Mutex::new(RunWatch::default())),
    }))
}

/// Streams the run's events through the writer, honouring the profile's
/// wall-clock cap; the runtime is scoped to this call.
fn drive_run(
    mounted: MountedRun,
    redactor: &craft_redact::Redactor,
) -> Result<(crate::RunOutcome, RunCaps), AppError> {
    let MountedRun {
        runner,
        prompt,
        caps,
        deadline,
        pool,
        format,
        session_path,
        watch,
    } = mounted;
    let (sink, writer) = spawn_run_writer(format, redactor)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| AppError::AsyncRuntime { source })?;
    let outcome = {
        let driven = async {
            let run_future = runner.run(&prompt, |mut event| {
                craft_runtime::redact_event(&mut event, redactor);
                RunWatch::record(&watch, &event);
                if matches!(event, RuntimeEvent::RunFinished { .. }) {
                    sink.finish(event);
                } else {
                    sink.emit(event);
                }
            });
            tokio::pin!(run_future);
            let kill = async {
                match deadline {
                    Some(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::pin!(kill);
            let outcome = tokio::select! {
                biased;
                outcome = &mut run_future => outcome,
                () = &mut kill => {
                    let (outcome, event) =
                        RunWatch::expire(&watch, deadline.unwrap_or(0), session_path.as_deref());
                    sink.finish(event);
                    outcome
                }
            };
            // A detached pool outlives its parent turn, so a run that
            // ends with children in flight drains them here: bounded,
            // and every straggler settles typed rather than vanishing.
            if let Some(wiring) = &pool {
                craft_runtime::drain_detached(wiring, craft_runtime::DETACHED_DRAIN).await;
            }
            outcome
        };
        runtime.block_on(event_writer::drive(driven, writer, session_path.clone()))
    };
    Ok((outcome, caps))
}

fn execute_run_inner(run: RunCli, redactor: &Arc<craft_redact::Redactor>) -> Result<i32, AppError> {
    let env = env_from_process();
    let config = resolve_env_config(&env)?;
    let Some(mounted) = mount_run(&run, redactor, &config, &env)? else {
        return Ok(1);
    };
    let (outcome, caps) = drive_run(mounted, redactor)?;
    summary::emit_run_summary(
        &env,
        run.output_format,
        &outcome,
        summary::SummaryFacts {
            mission: "run",
            model: config.model.clone(),
            provider: summary::provider_name(config.provider).to_string(),
            turn_cap: Some(caps.turn_cap),
            token_budget: caps.token_budget,
            rows: redaction::summary_rows(redactor),
        },
    );
    Ok(outcome.exit_code())
}
