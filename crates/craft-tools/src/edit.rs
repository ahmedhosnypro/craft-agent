//! The edit_file tool: prior-read required, exact-match replacement on
//! bytes (so binary-ish files are never mangled), changed-on-disk
//! detection via a content stamp.

use serde_json::Value;

use crate::error::ToolError;
use crate::event::{self, ToolStream};
use crate::pathguard::{resolve_within, sensitive_reason};
use crate::schema::{Prop, Schema};
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::util;

pub const EDIT_PROPS: &[Prop] = &[
    Prop {
        name: "path",
        ty: Schema::Str,
        required: true,
        desc: "File to edit; must have been read this session first.",
    },
    Prop {
        name: "old_string",
        ty: Schema::Str,
        required: true,
        desc: "The exact byte sequence to replace.",
    },
    Prop {
        name: "new_string",
        ty: Schema::Str,
        required: true,
        desc: "The replacement.",
    },
    Prop {
        name: "replace_all",
        ty: Schema::Bool,
        required: false,
        desc: "Replace every occurrence instead of requiring a unique match.",
    },
];

pub struct EditFileTool {
    limits: ToolLimits,
}

impl EditFileTool {
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }

    fn try_execute(&self, args: &Value, ctx: &ToolCtx) -> Result<ToolStream, ToolError> {
        util::reject_unknown_keys(
            args,
            "edit_file",
            &["path", "old_string", "new_string", "replace_all"],
        )?;
        let input = util::arg_str_bounded(args, "edit_file", "path", self.limits.max_path_bytes)?;
        let old = util::arg_str(args, "edit_file", "old_string")?;
        let new = util::arg_str(args, "edit_file", "new_string")?;
        let replace_all = util::arg_opt_bool(args, "edit_file", "replace_all")?.unwrap_or(false);
        if old.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "edit_file".to_string(),
                msg: "old_string must not be empty".to_string(),
            });
        }
        let abs = resolve_within(
            "edit_file",
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
        let Some(stamp) = ctx.read_stamp(&abs) else {
            return Err(ToolError::NotReadFirst {
                path: display(&abs),
            });
        };
        let bytes = std::fs::read(&abs).map_err(|e| ToolError::Io {
            op: "read",
            msg: e.to_string(),
        })?;
        if bytes.len() > self.limits.write_cap {
            return Err(ToolError::FileTooLarge {
                path: display(&abs),
                size: bytes.len() as u64,
                limit: self.limits.write_cap,
            });
        }
        if util::content_hash(&bytes) != stamp {
            return Err(ToolError::ChangedOnDisk {
                path: display(&abs),
            });
        }
        let positions = find_all(&bytes, old.as_bytes());
        if positions.is_empty() {
            return Err(ToolError::EditNoMatch {
                path: display(&abs),
            });
        }
        if positions.len() > 1 && !replace_all {
            return Err(ToolError::EditAmbiguous {
                path: display(&abs),
                count: positions.len(),
            });
        }
        let edited = splice(&bytes, old.as_bytes(), new.as_bytes(), &positions);
        std::fs::write(&abs, &edited).map_err(|e| ToolError::Io {
            op: "write",
            msg: e.to_string(),
        })?;
        ctx.mark_read(abs.clone(), util::content_hash(&edited));
        let note = format!("replaced {} occurrence(s) in {input}", positions.len());
        Ok(event::result_stream(note))
    }
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Non-overlapping match positions of `needle` in `hay`, ascending.
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = hay[cursor..]
        .windows(needle.len())
        .position(|w| w == needle)
    {
        out.push(cursor + rel);
        cursor = cursor + rel + needle.len();
        if hay.len() < cursor + needle.len() {
            break;
        }
    }
    out
}

fn splice(bytes: &[u8], old: &[u8], new: &[u8], positions: &[usize]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut last = 0usize;
    for pos in positions {
        out.extend_from_slice(&bytes[last..*pos]);
        out.extend_from_slice(new);
        last = pos + old.len();
    }
    out.extend_from_slice(&bytes[last..]);
    out
}

impl Tool for EditFileTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Replaces an exact byte match in a previously read file; aborts if the file changed on disk."
    }

    fn params_schema(&self) -> Schema {
        Schema::object("Applies one exact-match edit.", EDIT_PROPS)
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.try_execute(&args, ctx) {
            Ok(stream) => stream,
            Err(err) => event::error_stream(err),
        }
    }
}
