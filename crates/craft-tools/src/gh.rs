//! The gh tool: shells out to the preinstalled gh CLI with a scrubbed
//! env (GH_TOKEN passes through). `--json` calls must return parseable
//! JSON; every failure maps to a typed error.

use futures::StreamExt;
use serde_json::Value;

use crate::error::ToolError;
use crate::event::{self, ToolEvent, ToolProgress, ToolStream};
use crate::proc::{self, ProcEvent, ProcSpec, StreamTag};
use crate::schema::{Prop, Schema};
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::util;

pub const GH_PROPS: &[Prop] = &[Prop {
    name: "args",
    ty: Schema::StrList,
    required: true,
    desc: "gh CLI arguments, e.g. [\"pr\", \"list\", \"-R\", \"owner/repo\"]. GH_TOKEN is set for the child.",
}];

const JSON_SNIPPET: usize = 200;

pub struct GhTool {
    limits: ToolLimits,
}

impl GhTool {
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }

    fn try_execute(&self, args: &Value, ctx: &ToolCtx) -> Result<ToolStream, ToolError> {
        util::reject_unknown_keys(args, "gh", &["args"])?;
        let argv = util::arg_str_list(
            args,
            "gh",
            "args",
            self.limits.max_path_bytes,
            self.limits.max_args,
        )?;
        let wants_json = argv
            .iter()
            .any(|a| a == "--json" || a.starts_with("--json="));
        let spec = ProcSpec {
            program: "gh".to_string(),
            argv,
            env: ctx.child_env(),
            cwd: ctx.workspace_root().to_path_buf(),
            timeout: self.limits.gh_timeout,
            term_grace: self.limits.proc_grace,
            out_cap: self.limits.proc_out_cap,
        };
        let raw = proc::spawn(spec)?;
        let timeout_ms = u64::try_from(self.limits.gh_timeout.as_millis()).unwrap_or(u64::MAX);
        Ok(raw
            .map(move |item| item.and_then(|ev| map_event(ev, wants_json, timeout_ms)))
            .boxed())
    }
}

fn map_event(ev: ProcEvent, wants_json: bool, timeout_ms: u64) -> Result<ToolEvent, ToolError> {
    match ev {
        ProcEvent::Chunk { stream, text } => Ok(ToolEvent::Progress(match stream {
            StreamTag::Stdout => ToolProgress::stdout(text),
            StreamTag::Stderr => ToolProgress::stderr(text),
        })),
        ProcEvent::Exit(exit) => {
            if exit.timed_out {
                return Err(ToolError::TimedOut { timeout_ms });
            }
            match exit.code {
                Some(0) => {
                    if wants_json {
                        let trimmed = exit.stdout.trim();
                        if serde_json::from_str::<serde_json::Value>(trimmed).is_err() {
                            return Err(ToolError::GhBadJson {
                                snippet: util::truncate_char_boundary(trimmed, JSON_SNIPPET)
                                    .to_string(),
                            });
                        }
                    }
                    Ok(ToolEvent::Terminal(event::ToolResult {
                        output: compose(
                            exit.stdout,
                            exit.stdout_truncated || exit.stderr_truncated,
                        ),
                    }))
                }
                code => Err(ToolError::GhFailed {
                    code,
                    stderr: util::truncate_char_boundary(&exit.stderr, 512).to_string(),
                }),
            }
        }
    }
}

fn compose(stdout: String, truncated: bool) -> String {
    let mut out = stdout;
    if truncated {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(crate::TRUNCATION_MARKER);
        out.push('\n');
    }
    out
}

impl Tool for GhTool {
    fn name(&self) -> &'static str {
        "gh"
    }

    fn description(&self) -> &'static str {
        "Runs a gh CLI command with GH_TOKEN; --json output must parse. All GitHub I/O goes through this tool."
    }

    fn params_schema(&self) -> Schema {
        Schema::object("Runs one gh command.", GH_PROPS)
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.try_execute(&args, ctx) {
            Ok(stream) => stream,
            Err(err) => event::error_stream(err),
        }
    }
}
