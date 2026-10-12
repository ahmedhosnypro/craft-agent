//! The edit_file tool: prior-read required, exact-match replacement on
//! bytes (so binary-ish files are never mangled), changed-on-disk
//! detection via a content stamp, and a pure-CRLF view model: CRLF
//! files match on a collapsed LF view and write back CRLF, while
//! mixed-ending files are refused.

use serde_json::Value;

use crate::error::ToolError;
use crate::event::{self, ToolStream};
use crate::lineend::{self, Endings};
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
        desc: "The exact text to replace. Pure CRLF files are matched on an LF view, so LF or CRLF spellings both match.",
    },
    Prop {
        name: "new_string",
        ty: Schema::Str,
        required: true,
        desc: "The replacement. Pure CRLF files are written back with CRLF, and a CR is never doubled.",
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
        if old == new {
            return Err(ToolError::InvalidArgs {
                tool: "edit_file".to_string(),
                msg: "old_string and new_string are exactly the same".to_string(),
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
        let style = lineend::classify(&bytes);
        if style == Endings::Mixed {
            return Err(ToolError::EditMixedEndings {
                path: display(&abs),
            });
        }
        let file_view = lineend::view(&bytes, style);
        let old_view = lineend::view(old.as_bytes(), style);
        let new_view = lineend::view(new.as_bytes(), style);
        let positions = find_all(&file_view, &old_view);
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
        let spliced = self.checked_splice(&abs, &file_view, &old_view, &new_view, &positions)?;
        let edited = lineend::materialize(&spliced, style);
        if edited.len() > self.limits.write_cap {
            return Err(ToolError::FileTooLarge {
                path: display(&abs),
                size: edited.len() as u64,
                limit: self.limits.write_cap,
            });
        }
        std::fs::write(&abs, &edited).map_err(|e| ToolError::Io {
            op: "write",
            msg: e.to_string(),
        })?;
        ctx.mark_read(abs.clone(), util::content_hash(&edited));
        let note = format!("replaced {} occurrence(s) in {input}", positions.len());
        Ok(event::result_stream(note))
    }

    /// Splice the replacements in, rejecting a result that would exceed
    /// the write cap before anything is allocated or written.
    fn checked_splice(
        &self,
        abs: &std::path::Path,
        file_view: &[u8],
        old_view: &[u8],
        new_view: &[u8],
        positions: &[usize],
    ) -> Result<Vec<u8>, ToolError> {
        let count = positions.len();
        let out_len = file_view
            .len()
            .saturating_sub(count.saturating_mul(old_view.len()))
            .saturating_add(count.saturating_mul(new_view.len()));
        if out_len > self.limits.write_cap {
            return Err(ToolError::FileTooLarge {
                path: display(abs),
                size: out_len as u64,
                limit: self.limits.write_cap,
            });
        }
        Ok(splice(file_view, old_view, new_view, positions))
    }
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Non-overlapping match positions of `needle` in `hay`, ascending.
/// Knuth-Morris-Pratt, so long partial matches cannot make the scan
/// quadratic in hostile input.
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }
    let mut fail = vec![0usize; needle.len()];
    let mut matched = 0usize;
    for i in 1..needle.len() {
        while matched > 0 && needle.get(i) != needle.get(matched) {
            matched = fail.get(matched - 1).copied().unwrap_or(0);
        }
        if needle.get(i) == needle.get(matched) {
            matched += 1;
        }
        if let Some(slot) = fail.get_mut(i) {
            *slot = matched;
        }
    }
    let mut out = Vec::new();
    let mut matched = 0usize;
    let mut cursor = 0usize;
    while let Some(byte) = hay.get(cursor) {
        if matched == needle.len() {
            out.push(cursor - needle.len());
            matched = 0;
            continue;
        }
        if Some(byte) == needle.get(matched) {
            matched += 1;
            cursor += 1;
        } else if matched > 0 {
            matched = fail.get(matched - 1).copied().unwrap_or(0);
        } else {
            cursor += 1;
        }
    }
    if matched == needle.len() {
        out.push(hay.len() - needle.len());
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
        "Replaces an exact match in a previously read file; pure CRLF files match on an LF view and write back CRLF; files with mixed line endings are refused; aborts if the file changed on disk."
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
