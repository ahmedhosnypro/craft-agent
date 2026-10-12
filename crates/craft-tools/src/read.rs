//! The read_file tool: exact content within a size guard, stamping the
//! read for edit_file's prior-read and changed-on-disk guards.

use std::io::Read;

use serde_json::Value;

use crate::error::ToolError;
use crate::event::{self, ToolStream};
use crate::pathguard::{resolve_read, sensitive_reason};
use crate::schema::{Prop, Schema};
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::util;

pub const READ_PROPS: &[Prop] = &[Prop {
    name: "path",
    ty: Schema::Str,
    required: true,
    desc: "File path relative to the workspace root, or absolute inside it or a configured read root.",
}];

pub struct ReadFileTool {
    limits: ToolLimits,
}

impl ReadFileTool {
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }

    fn try_execute(&self, args: &Value, ctx: &ToolCtx) -> Result<ToolStream, ToolError> {
        util::reject_unknown_keys(args, "read_file", &["path"])?;
        let input = util::arg_str_bounded(args, "read_file", "path", self.limits.max_path_bytes)?;
        let abs = resolve_read(
            "read_file",
            ctx.workspace_root(),
            ctx.read_roots(),
            &input,
            self.limits.max_path_bytes,
        )?;
        if let Some(reason) = sensitive_reason(&abs) {
            return Err(ToolError::SensitivePath {
                path: display(&abs),
                reason,
            });
        }
        let meta = std::fs::metadata(&abs).map_err(|e| meta_err(&abs, e))?;
        if !meta.is_file() {
            return Err(ToolError::Io {
                op: "read",
                msg: "not a regular file".to_string(),
            });
        }
        if meta.len() > self.limits.read_cap as u64 {
            return Err(ToolError::FileTooLarge {
                path: display(&abs),
                size: meta.len(),
                limit: self.limits.read_cap,
            });
        }
        let mut bytes = Vec::new();
        let budget = self.limits.read_cap as u64 + 1;
        let file = std::fs::File::open(&abs).map_err(|e| meta_err(&abs, e))?;
        file.take(budget)
            .read_to_end(&mut bytes)
            .map_err(|e| ToolError::Io {
                op: "read",
                msg: e.to_string(),
            })?;
        if bytes.len() > self.limits.read_cap {
            return Err(ToolError::FileTooLarge {
                path: display(&abs),
                size: bytes.len() as u64,
                limit: self.limits.read_cap,
            });
        }
        ctx.mark_read(abs.clone(), util::content_hash(&bytes));
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok(event::result_stream(text))
    }
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn meta_err(path: &std::path::Path, e: std::io::Error) -> ToolError {
    if e.kind() == std::io::ErrorKind::NotFound {
        ToolError::NotFound {
            path: display(path),
        }
    } else {
        ToolError::Io {
            op: "read",
            msg: e.to_string(),
        }
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Reads a UTF-8 text file inside the workspace or a configured read root; content above the size guard is rejected."
    }

    fn params_schema(&self) -> Schema {
        Schema::object("Reads one file.", READ_PROPS)
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.try_execute(&args, ctx) {
            Ok(stream) => stream,
            Err(err) => event::error_stream(err),
        }
    }
}
