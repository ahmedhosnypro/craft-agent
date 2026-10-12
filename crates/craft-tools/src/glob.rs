//! The glob tool: bounded, sorted file listing via globset over the
//! deterministic walker.

use globset;
use serde_json::Value;

use crate::error::ToolError;
use crate::event::{self, ToolStream};
use crate::pathguard::{resolve_read, sensitive_reason};
use crate::schema::{Prop, Schema};
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::util;
use crate::walk::walk_files;

pub const GLOB_PROPS: &[Prop] = &[
    Prop {
        name: "pattern",
        ty: Schema::Str,
        required: true,
        desc: "Glob pattern matched against paths relative to the search root (`*`, `**`, `?`).",
    },
    Prop {
        name: "path",
        ty: Schema::Str,
        required: false,
        desc: "Directory to search instead of the workspace root; an absolute configured read root also works. Workspace results are root-relative; read-root results are absolute.",
    },
];

pub struct GlobTool {
    limits: ToolLimits,
}

impl GlobTool {
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }

    fn try_execute(&self, args: &Value, ctx: &ToolCtx) -> Result<ToolStream, ToolError> {
        util::reject_unknown_keys(args, "glob", &["pattern", "path"])?;
        let pattern =
            util::arg_str_bounded(args, "glob", "pattern", self.limits.max_pattern_bytes)?;
        if pattern.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "glob".to_string(),
                msg: "pattern must not be empty".to_string(),
            });
        }
        let scope_input = util::arg_opt_str(args, "glob", "path", self.limits.max_path_bytes)?
            .unwrap_or_else(|| ".".to_string());
        let root = ctx.workspace_root().to_path_buf();
        let scope = resolve_read(
            "glob",
            &root,
            ctx.read_roots(),
            &scope_input,
            self.limits.max_path_bytes,
        )?;
        if let Some(reason) = sensitive_reason(&scope) {
            return Err(ToolError::SensitivePath {
                path: display(&scope),
                reason,
            });
        }
        let matcher = globset::GlobBuilder::new(&pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| ToolError::InvalidArgs {
                tool: "glob".to_string(),
                msg: e.to_string(),
            })?
            .compile_matcher();

        let scope_meta = std::fs::metadata(&scope).map_err(|_| ToolError::NotFound {
            path: display(&scope),
        })?;

        let mut hits: Vec<String> = Vec::new();
        let outcome;
        if scope_meta.is_file() {
            outcome = crate::walk::WalkOutcome::default();
            let base = scope.parent().unwrap_or(&scope);
            if let Some(rel) = rel_to(&scope, base)
                && matcher.is_match(&rel)
            {
                hits.push(display_rel(&scope, &root));
            }
        } else {
            let limits = self.limits;
            outcome = walk_files(
                &scope,
                limits.walk_max_entries,
                limits.walk_max_depth,
                &mut |path| {
                    if let Some(rel) = rel_to(&path, &scope)
                        && matcher.is_match(&rel)
                    {
                        hits.push(display_rel(&path, &root));
                    }
                    hits.len() < limits.glob_max_results
                },
            )?;
        }
        hits.sort_unstable();
        let mut out = hits.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        if outcome.skipped_sensitive > 0 {
            out.push_str(&format!(
                "[skipped {} sensitive files]\n",
                outcome.skipped_sensitive
            ));
        }
        if hits.len() >= self.limits.glob_max_results {
            out.push_str(&format!(
                "[results capped at {}]\n",
                self.limits.glob_max_results
            ));
        }
        Ok(event::result_stream(out))
    }
}

fn rel_to(path: &std::path::Path, base: &std::path::Path) -> Option<String> {
    path.strip_prefix(base)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

fn display_rel(path: &std::path::Path, root: &std::path::Path) -> String {
    rel_to(path, root).unwrap_or_else(|| display(path))
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "glob"
    }

    fn description(&self) -> &'static str {
        "Lists workspace files matching a glob pattern, sorted, with a bounded result count; a configured read root is searchable too."
    }

    fn params_schema(&self) -> Schema {
        Schema::object("Lists files by glob.", GLOB_PROPS)
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.try_execute(&args, ctx) {
            Ok(stream) => stream,
            Err(err) => event::error_stream(err),
        }
    }
}
