//! The write_file tool: guarded writes with parent-directory creation.
//! A write does not satisfy edit_file's prior-read rule — only a read
//! does.

use serde_json::Value;

use crate::error::ToolError;
use crate::event::{self, ToolStream};
use crate::pathguard::{resolve_within, sensitive_reason};
use crate::schema::{Prop, Schema};
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::util;

pub const WRITE_PROPS: &[Prop] = &[
    Prop {
        name: "path",
        ty: Schema::Str,
        required: true,
        desc: "Target file path inside the workspace; missing parents are created.",
    },
    Prop {
        name: "content",
        ty: Schema::Str,
        required: true,
        desc: "The full file content to write.",
    },
];

pub struct WriteFileTool {
    limits: ToolLimits,
}

impl WriteFileTool {
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }

    fn try_execute(&self, args: &Value, ctx: &ToolCtx) -> Result<ToolStream, ToolError> {
        util::reject_unknown_keys(args, "write_file", &["path", "content"])?;
        let input = util::arg_str_bounded(args, "write_file", "path", self.limits.max_path_bytes)?;
        let content = util::arg_str(args, "write_file", "content")?;
        if content.len() > self.limits.write_cap {
            return Err(ToolError::WriteTooLarge {
                size: content.len(),
                limit: self.limits.write_cap,
            });
        }
        let abs = resolve_within(
            "write_file",
            ctx.workspace_root(),
            &input,
            self.limits.max_path_bytes,
        )?;
        if let Some(reason) = sensitive_reason(&abs) {
            return Err(ToolError::SensitivePath {
                path: display(&abs),
                reason,
            });
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ToolError::Io {
                op: "create directories",
                msg: e.to_string(),
            })?;
            let canon = std::fs::canonicalize(parent).map_err(|e| ToolError::Io {
                op: "create directories",
                msg: e.to_string(),
            })?;
            let canon_root = std::fs::canonicalize(ctx.workspace_root())
                .unwrap_or_else(|_| ctx.workspace_root().to_path_buf());
            if !canon.starts_with(&canon_root) {
                return Err(ToolError::EscapesWorkspace {
                    path: display(&abs),
                });
            }
        }
        std::fs::write(&abs, content.as_bytes()).map_err(|e| ToolError::Io {
            op: "write",
            msg: e.to_string(),
        })?;
        let note = format!("wrote {} bytes to {input}", content.len());
        Ok(event::result_stream(note))
    }
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Tool for WriteFileTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Writes a full file inside the workspace; missing parent directories are created."
    }

    fn params_schema(&self) -> Schema {
        Schema::object("Writes one file.", WRITE_PROPS)
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.try_execute(&args, ctx) {
            Ok(stream) => stream,
            Err(err) => event::error_stream(err),
        }
    }
}
