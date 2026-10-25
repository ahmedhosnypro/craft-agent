//! `workflow`: locate a plan, pre-resolve its agent profiles, drive the
//! engine over one bounded run per step, stream the two workflow
//! events, and exit with the report's code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use craft_core::agents::{AgentFile, CatalogHit, load_catalog};
use craft_providers::backend::Backend;
use craft_runtime::{
    DEFAULT_REQUEST_TOKENS, RuntimeEvent, SubagentCaps, SubagentHub, SubagentSettings,
    SubagentWiring,
};
use craft_tools::ToolLimits;
use craft_workflow::plan::{ArgKind, ArgValue};
use craft_workflow::run::{args_digest, run_id_for};
use craft_workflow::{Journal, WorkflowError, WorkflowPlan, plan, resume_workflow, run_workflow};

use crate::cli::WorkflowCli;
use crate::config::{EnvConfig, env_from_process, mount_backend, resolve_env_config};
use crate::error::AppError;
use crate::event_writer;
use crate::prompt::DEFAULT_SYSTEM_PROMPT;
use crate::redaction;
use crate::stream_json;
use crate::summary;
use crate::workflow_exec::WorkflowExecutor;
use crate::workflow_locate::{listing, locate_plan};

/// Raw KEY=VALUE flags onto declared arg values: a string arg keeps
/// the value verbatim, an array arg splits it on commas. Undeclared
/// keys pass through for `resolve_args` to reject by name.
pub fn convert_arg_flags(
    plan_spec: &WorkflowPlan,
    raw: &[(String, String)],
) -> Result<BTreeMap<String, ArgValue>, AppError> {
    let mut provided = BTreeMap::new();
    for (key, value) in raw {
        let value = match plan_spec.args.get(key).map(|spec| spec.kind) {
            Some(ArgKind::Array) if value.is_empty() => ArgValue::Seq(Vec::new()),
            Some(ArgKind::Array) => ArgValue::Seq(
                value
                    .split(',')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect(),
            ),
            _ => ArgValue::Str(value.clone()),
        };
        if provided.insert(key.clone(), value).is_some() {
            return Err(AppError::WorkflowArgDuplicate { key: key.clone() });
        }
    }
    Ok(provided)
}

/// Resolves every step's agent profile up front, fail-closed: a plan
/// whose persona is missing never starts a step.
pub fn resolve_step_agents(
    plan_spec: &WorkflowPlan,
    workspace_root: &Path,
) -> Result<BTreeMap<String, AgentFile>, AppError> {
    let catalog =
        load_catalog(workspace_root).map_err(|source| AppError::WorkflowCatalog { source })?;
    for skip in &catalog.skipped {
        eprintln!(
            "craft-agent: skipped agent file `{}`: {}",
            skip.key, skip.error
        );
    }
    let mut agents = BTreeMap::new();
    for spec in &plan_spec.steps {
        let agent = match catalog.lookup(&spec.agent) {
            CatalogHit::Found(agent) => agent.clone(),
            CatalogHit::Invalid(error) => {
                return Err(AppError::WorkflowAgentFile {
                    step: spec.id.clone(),
                    name: spec.agent.clone(),
                    source: error.clone(),
                });
            }
            CatalogHit::Missing => {
                return Err(AppError::WorkflowAgentMissing {
                    step: spec.id.clone(),
                    name: spec.agent.clone(),
                    available: listing(catalog.names().into_iter().map(String::from)),
                });
            }
        };
        agents.insert(spec.id.clone(), agent);
    }
    Ok(agents)
}

/// The workflow run's subagent hub: mounted once per run over the
/// default model's backend and shared by every step, so a fan-out
/// step's registry carries the spawn pair while the run's caps,
/// catalog, and token pool bind the whole tree. A mount failure
/// aborts the run — a step must never silently degrade to serial
/// work because its hub was missing.
fn mount_workflow_hub(
    env_config: &EnvConfig,
    workspace_root: &Path,
    run_token_cap: Option<u64>,
    idle_timeout: Option<Duration>,
    sessions_dir: &Path,
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

/// The run-or-resume switch over one journal: a fresh journal runs, a
/// non-empty one resumes — a finished journal replays purely.
pub async fn run_or_resume<E: craft_workflow::engine::StepExecutor + Sync>(
    plan_spec: &WorkflowPlan,
    args: &BTreeMap<String, ArgValue>,
    executor: &E,
    journal: &mut Journal,
) -> Result<craft_workflow::WorkflowReport, WorkflowError> {
    if journal.is_empty() {
        run_workflow(plan_spec, args, executor, journal).await
    } else {
        resume_workflow(plan_spec, args, executor, journal).await
    }
}

/// The workflow's run directory: `<root>/.craft-agent/workflows/<run_id>`.
pub fn workflow_run_dir(workspace_root: &Path, run_id: &str) -> PathBuf {
    workspace_root
        .join(".craft-agent")
        .join("workflows")
        .join(run_id)
}

pub fn execute_workflow(cli: WorkflowCli) -> Result<i32, AppError> {
    let format = cli.output_format;
    let redactor = std::sync::Arc::new(craft_redact::Redactor::from_env());
    let env = env_from_process();
    redaction::emit_add_mask(&redactor, &env);
    execute_workflow_inner(cli, &redactor).inspect_err(|error| {
        stream_json::write_terminal_line(
            std::io::stdout().lock(),
            format,
            redaction::terminal_detail(error, &redactor),
        );
    })
}

fn execute_workflow_inner(
    cli: WorkflowCli,
    redactor: &std::sync::Arc<craft_redact::Redactor>,
) -> Result<i32, AppError> {
    let env = env_from_process();
    let workspace_root =
        std::env::current_dir().map_err(|source| AppError::WorkspaceRoot { source })?;
    let plan_path = locate_plan(&cli, &env, &workspace_root)?;
    let plan_spec =
        WorkflowPlan::parse_file(&plan_path).map_err(|source| AppError::Workflow { source })?;
    let provided = convert_arg_flags(&plan_spec, &cli.args)?;
    let args = plan::resolve_args(&plan_spec, &provided)
        .map_err(|source| AppError::Workflow { source })?;
    let digest = args_digest(&args).map_err(|source| AppError::Workflow { source })?;
    let run_id = run_id_for(&plan_spec.plan_hash, &digest);
    let env_config = resolve_env_config(&env)?;
    let agents = resolve_step_agents(&plan_spec, &workspace_root)?;
    let run_dir = workflow_run_dir(&workspace_root, &run_id);
    std::fs::create_dir_all(run_dir.join("sessions")).map_err(|source| {
        AppError::WorkflowRunDir {
            path: run_dir.clone(),
            source,
        }
    })?;
    let journal = Journal::open(&run_dir.join("journal.jsonl"))
        .map_err(|source| AppError::Workflow { source })?;
    let sessions_dir = run_dir.join("sessions");
    let run_token_cap = Some(cli.max_tokens.unwrap_or(plan_spec.limits.max_total_tokens));
    let idle_timeout = cli.idle_timeout_secs.map(Duration::from_secs);
    let hub = mount_workflow_hub(
        &env_config,
        &workspace_root,
        run_token_cap,
        idle_timeout,
        &sessions_dir,
    )?;
    let (sink, writer) = spawn_workflow_writer(cli.output_format, redactor)?;
    // The executor holds its own sink clone: a step's subagent launches
    // and settles ride the same event stream the workflow frames do.
    let step_sink = sink.clone();
    let executor = WorkflowExecutor::new(
        production_mount(env_config.clone()),
        env_config.model.clone(),
        workspace_root.clone(),
        sessions_dir,
        run_token_cap,
        idle_timeout,
        None,
        agents,
    )
    .with_subagents(Some(hub.clone()))
    .with_sink(Some(step_sink));
    sink.emit(RuntimeEvent::WorkflowStarted {
        run_id: run_id.clone(),
        name: plan_spec.name.clone(),
        plan_hash: plan_spec.plan_hash.clone(),
        args_digest: digest.clone(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| AppError::AsyncRuntime { source })?;
    let report = runtime.block_on(drive_workflow(
        &plan_spec, &args, &executor, journal, &sink, writer, hub,
    ))?;
    summary::emit_workflow_summary(
        &env,
        cli.output_format,
        summary::WorkflowRun {
            report: &report,
            sessions_dir: &run_dir.join("sessions"),
        },
        summary::SummaryFacts {
            mission: "workflow",
            model: env_config.model.clone(),
            provider: summary::provider_name(env_config.provider).to_string(),
            turn_cap: None,
            token_budget: Some(cli.max_tokens.unwrap_or(plan_spec.limits.max_total_tokens)),
            rows: redaction::summary_rows(redactor),
        },
    );
    Ok(report.exit_code)
}

/// Spawns the run-event writer; on failure the workflow still ends with
/// its terminal stream line, written straight to stdout.
fn spawn_workflow_writer(
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

/// Mounts the step's model over the resolved provider configuration.
pub fn production_mount(config: EnvConfig) -> crate::workflow_exec::StepMount<Backend> {
    std::sync::Arc::new(move |model| {
        let step_config = EnvConfig {
            model: model.to_string(),
            ..config.clone()
        };
        mount_backend(&step_config).map_err(|err| err.to_string())
    })
}

enum DriveEnd {
    Report(craft_workflow::WorkflowReport),
    Failed(WorkflowError),
    Closed,
}

/// Drives the engine to its terminal report, ending the run the
/// moment the event-stream consumer goes away, then drains the run's
/// detached pool and emits the `workflow_finished` line.
pub async fn drive_workflow<E: craft_workflow::engine::StepExecutor + Sync>(
    plan_spec: &WorkflowPlan,
    args: &BTreeMap<String, ArgValue>,
    executor: &E,
    mut journal: Journal,
    sink: &crate::event_writer::EventSink,
    writer: crate::event_writer::EventWriterHandle,
    hub: SubagentWiring,
) -> Result<craft_workflow::WorkflowReport, AppError> {
    let mut closed = writer.closed().clone();
    let _ = closed.borrow_and_update();
    let driven = run_or_resume(plan_spec, args, executor, &mut journal);
    let end = tokio::select! {
        biased;
        outcome = driven => match outcome {
            Ok(report) => DriveEnd::Report(report),
            Err(err) => DriveEnd::Failed(err),
        },
        _ = closed.changed() => DriveEnd::Closed,
    };
    // The pool outlives its step, so a run ending with detached
    // children in flight drains them here: bounded, and every straggler
    // settles typed rather than vanishing from the ledger.
    craft_runtime::drain_detached(&hub, craft_runtime::DETACHED_DRAIN).await;
    let report = match end {
        DriveEnd::Report(report) => report,
        DriveEnd::Failed(err) => {
            writer.drain().await;
            return Err(AppError::Workflow { source: err });
        }
        DriveEnd::Closed => {
            writer.drain().await;
            return Err(AppError::WorkflowStreamClosed);
        }
    };
    let payload =
        serde_json::to_value(&report).map_err(|source| AppError::WorkflowReportJson { source })?;
    sink.finish(RuntimeEvent::WorkflowFinished {
        exit_code: report.exit_code,
        report: payload,
    });
    writer.drain().await;
    Ok(report)
}
