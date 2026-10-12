//! The bash tool: runs a script via `bash -c` with a scrubbed child env,
//! a bounded timeout, and bounded streamed output.

use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;

use crate::egress;
use crate::error::ToolError;
use crate::event::{self, ToolEvent, ToolProgress, ToolStream};
use crate::proc::{self, ProcEvent, ProcSpec, StreamTag};
use crate::schema::{Prop, Schema};
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::util;

pub const BASH_PROPS: &[Prop] = &[
    Prop {
        name: "command",
        ty: Schema::Str,
        required: true,
        desc: "The bash script to run. Stdin is closed; combined output is capped at 256 KiB.",
    },
    Prop {
        name: "timeout_secs",
        ty: Schema::Int,
        required: false,
        desc: "Optional wall-clock cap in seconds. Requests above the maximum are rejected.",
    },
];

pub struct BashTool {
    limits: ToolLimits,
}

impl BashTool {
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }

    fn try_execute(&self, args: &Value, ctx: &ToolCtx) -> Result<ToolStream, ToolError> {
        util::reject_unknown_keys(args, "bash", &["command", "timeout_secs"])?;
        let command =
            util::arg_str_bounded(args, "bash", "command", self.limits.max_command_bytes)?;
        if let Some(hosts) = egress::allowlist(ctx.env_value(egress::KNOB).as_deref()) {
            egress::check(&command, &hosts)?;
        }
        let max_secs = self.limits.bash_max_timeout.as_secs();
        let timeout_secs = util::arg_opt_u64(args, "bash", "timeout_secs", max_secs)?;
        let timeout = match timeout_secs {
            None => self.limits.bash_default_timeout,
            Some(secs) => Duration::from_secs(secs),
        };
        let spec = ProcSpec {
            program: "bash".to_string(),
            argv: vec!["-c".to_string(), command],
            env: ctx.child_env(),
            cwd: ctx.workspace_root().to_path_buf(),
            timeout,
            term_grace: self.limits.proc_grace,
            out_cap: self.limits.proc_out_cap,
        };
        let raw = proc::spawn(spec)?;
        let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
        let out_cap = self.limits.proc_out_cap;
        Ok(raw
            .map(move |item| item.and_then(|ev| map_event(ev, timeout_ms, out_cap)))
            .boxed())
    }
}

fn map_event(ev: ProcEvent, timeout_ms: u64, out_cap: usize) -> Result<ToolEvent, ToolError> {
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
                Some(0) => Ok(ToolEvent::Terminal(event::ToolResult {
                    output: compose(exit.stdout, exit.stderr, out_cap),
                })),
                code => Err(ToolError::NonZeroExit {
                    code,
                    stderr: snippet(&exit.stderr),
                }),
            }
        }
    }
}

/// Compose the terminal output: stdout, then stderr under a label, then
/// the truncation marker if either side was cut.
fn compose(stdout: String, stderr: String, out_cap: usize) -> String {
    let mut out = stdout;
    if !stderr.is_empty() {
        out.push_str("\n[stderr]\n");
        out.push_str(&stderr);
    }
    if out.len() + crate::TRUNCATION_MARKER.len() > out_cap {
        let keep = out_cap.saturating_sub(crate::TRUNCATION_MARKER.len() + 1);
        out = util::truncate_char_boundary(&out, keep).to_string();
        out.push('\n');
        out.push_str(crate::TRUNCATION_MARKER);
    }
    out
}

fn snippet(stderr: &str) -> String {
    util::truncate_char_boundary(stderr, 512).to_string()
}

impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Runs a bash script in the workspace with closed stdin, a bounded timeout, \
         and a 256 KiB output cap; nonzero exits and timeouts are reported as typed \
         errors. Use it for commands (git, gh, jq), not for reading files — \
         read_file, glob, and grep are cheaper and structured."
    }

    fn params_schema(&self) -> Schema {
        Schema::object("Runs a shell command.", BASH_PROPS)
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.try_execute(&args, ctx) {
            Ok(stream) => stream,
            Err(err) => event::error_stream(err),
        }
    }
}
