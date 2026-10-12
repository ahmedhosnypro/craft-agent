//! The grep tool: Rust-native content search — the regex crate over the
//! bounded deterministic walker. No external search binary is invoked.

use globset;
use regex::Regex;
use serde_json::Value;

use crate::error::ToolError;
use crate::event::{self, ToolStream};
use crate::pathguard::{resolve_within, sensitive_reason};
use crate::schema::{Prop, Schema};
use crate::tool::{Tool, ToolCtx, ToolLimits};
use crate::util;
use crate::walk::walk_files;

pub const GREP_PROPS: &[Prop] = &[
    Prop {
        name: "pattern",
        ty: Schema::Str,
        required: true,
        desc: "Text to find; a regex unless `literal` is true.",
    },
    Prop {
        name: "path",
        ty: Schema::Str,
        required: false,
        desc: "File or directory to search instead of the workspace root.",
    },
    Prop {
        name: "glob",
        ty: Schema::Str,
        required: false,
        desc: "Only files matching this glob pattern are searched.",
    },
    Prop {
        name: "literal",
        ty: Schema::Bool,
        required: false,
        desc: "Treat the pattern as literal text instead of a regex.",
    },
];

const BINARY_PROBE: usize = 8 * 1024;

pub struct GrepTool {
    limits: ToolLimits,
}
impl GrepTool {
    pub fn new(limits: ToolLimits) -> Self {
        Self { limits }
    }

    fn try_execute(&self, args: &Value, ctx: &ToolCtx) -> Result<ToolStream, ToolError> {
        util::reject_unknown_keys(args, "grep", &["pattern", "path", "glob", "literal"])?;
        let pattern =
            util::arg_str_bounded(args, "grep", "pattern", self.limits.max_pattern_bytes)?;
        if pattern.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "grep".to_string(),
                msg: "pattern must not be empty".to_string(),
            });
        }
        let literal = util::arg_opt_bool(args, "grep", "literal")?.unwrap_or(false);
        let needle = if literal {
            regex::escape(&pattern)
        } else {
            pattern.clone()
        };
        let matcher = Regex::new(&needle).map_err(|e| ToolError::BadPattern {
            msg: util::truncate_char_boundary(&e.to_string(), 512).to_string(),
        })?;
        let filter = match util::arg_opt_str(args, "grep", "glob", self.limits.max_pattern_bytes)? {
            Some(raw) => Some(
                globset::GlobBuilder::new(&raw)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| ToolError::InvalidArgs {
                        tool: "grep".to_string(),
                        msg: e.to_string(),
                    })?
                    .compile_matcher(),
            ),
            None => None,
        };
        let scope_input = util::arg_opt_str(args, "grep", "path", self.limits.max_path_bytes)?
            .unwrap_or_else(|| ".".to_string());
        let root = ctx.workspace_root().to_path_buf();
        let scope = resolve_within("grep", &root, &scope_input, self.limits.max_path_bytes)?;
        if let Some(reason) = sensitive_reason(&scope) {
            return Err(ToolError::SensitivePath {
                path: display(&scope),
                reason,
            });
        }
        let meta = std::fs::metadata(&scope).map_err(|_| ToolError::NotFound {
            path: display(&scope),
        })?;
        let mut report = Report::default();
        if meta.is_file() {
            let filter_base = scope.parent().unwrap_or(&scope);
            report.search_file(&scope, &root, filter_base, &matcher, &filter, self.limits);
        } else {
            let limits = self.limits;
            let outcome = walk_files(
                &scope,
                limits.walk_max_entries,
                limits.walk_max_depth,
                &mut |path| {
                    report.search_file(&path, &root, &scope, &matcher, &filter, limits);
                    !report.capped()
                },
            )?;
            report.skipped_sensitive = outcome.skipped_sensitive;
        }
        Ok(event::result_stream(report.render(&pattern, self.limits)))
    }
}

#[derive(Default)]
struct Report {
    rows: Vec<(String, usize, String)>,
    skipped_sensitive: usize,
    skipped_oversize: usize,
    skipped_binary: usize,
    skipped_unreadable: usize,
    capped: bool,
}

impl Report {
    fn capped(&self) -> bool {
        self.capped
    }

    /// Search one file: oversize, binary, and unreadable files are
    /// skipped and counted; matching lines become `path:line: excerpt` rows.
    fn search_file(
        &mut self,
        path: &std::path::Path,
        root: &std::path::Path,
        scope: &std::path::Path,
        matcher: &Regex,
        filter: &Option<globset::GlobMatcher>,
        limits: ToolLimits,
    ) {
        if let Some(f) = filter {
            let rel = match path.strip_prefix(scope) {
                Ok(r) => r.to_string_lossy().into_owned(),
                Err(_) => return,
            };
            if !f.is_match(&rel) {
                return;
            }
        }
        let Ok(meta) = std::fs::metadata(path) else {
            self.skipped_unreadable = self.skipped_unreadable.saturating_add(1);
            return;
        };
        if !meta.is_file() || meta.len() > limits.grep_file_cap as u64 {
            self.skipped_oversize = self.skipped_oversize.saturating_add(1);
            return;
        }
        let Ok(bytes) = std::fs::read(path) else {
            self.skipped_unreadable = self.skipped_unreadable.saturating_add(1);
            return;
        };
        if bytes.len() > limits.grep_file_cap || bytes[..bytes.len().min(BINARY_PROBE)].contains(&0)
        {
            self.skipped_binary = self.skipped_binary.saturating_add(1);
            return;
        }
        let text = String::from_utf8_lossy(&bytes);
        let display = match path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().into_owned(),
            Err(_) => path.to_string_lossy().into_owned(),
        };
        for (idx, line) in text.split_inclusive('\n').enumerate() {
            let body = line.strip_suffix('\n').unwrap_or(line);
            let Some(found) = matcher.find(body) else {
                continue;
            };
            let start = found.start().min(body.len());
            let tail = util::truncate_char_boundary(&body[start..], limits.grep_line_cap);
            self.rows.push((display.clone(), idx + 1, tail.to_string()));
            if self.rows.len() >= limits.grep_max_results {
                self.capped = true;
                return;
            }
        }
    }

    fn render(&mut self, pattern: &str, limits: ToolLimits) -> String {
        self.rows.sort_unstable();
        let mut out = String::new();
        for (path, line, tail) in &self.rows {
            out.push_str(&format!("{path}:{line}: {tail}\n"));
        }
        if self.skipped_oversize > 0 {
            out.push_str(&format!(
                "[skipped {} oversized files]\n",
                self.skipped_oversize
            ));
        }
        if self.skipped_binary > 0 {
            out.push_str(&format!("[skipped {} binary files]\n", self.skipped_binary));
        }
        if self.skipped_unreadable > 0 {
            out.push_str(&format!(
                "[skipped {} unreadable files]\n",
                self.skipped_unreadable
            ));
        }
        if self.skipped_sensitive > 0 {
            out.push_str(&format!(
                "[skipped {} sensitive files]\n",
                self.skipped_sensitive
            ));
        }
        if self.capped {
            out.push_str(&format!(
                "[results capped at {}]\n",
                limits.grep_max_results
            ));
        }
        if self.rows.is_empty() {
            out.push_str(&format!("no matches for `{pattern}`\n"));
        }
        out
    }
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Searches file contents with a regex or literal pattern across the workspace, bounded and sorted."
    }

    fn params_schema(&self) -> Schema {
        Schema::object("Searches file contents.", GREP_PROPS)
    }

    fn execute(&self, args: Value, ctx: &ToolCtx) -> ToolStream {
        match self.try_execute(&args, ctx) {
            Ok(stream) => stream,
            Err(err) => event::error_stream(err),
        }
    }
}
