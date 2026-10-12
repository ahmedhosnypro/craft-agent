//! Workspace path containment and the sensitive-path predicate.
//!
//! Reads may also reach the extra read roots named by
//! `CRAFT_AGENT_READ_ROOTS` — the workspace is always tried first,
//! then roots in declaration order, and symlink components resolve
//! against the matched root, so a link may not stitch roots. Sensitive
//! paths are denied on every call: the guard is a pure
//! function of the resolved path, so nothing is cached across calls.
//! Ban patterns match case-insensitively; the public exemptions
//! (`.env.example`/`.env.sample`/`.env.template`, `*.pub` keys) require
//! the exact documented spellings. Symlink components resolve against
//! the root: a link that leaves the root, dangles, or names a
//! sensitive target is denied.

use std::path::{Component, Path, PathBuf};

use crate::error::ToolError;

fn arg_err(tool: &str, msg: String) -> ToolError {
    ToolError::InvalidArgs {
        tool: tool.to_string(),
        msg,
    }
}

fn escapes(path: &str) -> ToolError {
    ToolError::EscapesWorkspace {
        path: path.to_string(),
    }
}

/// The env knob naming extra read-only roots for the file-read tools.
pub const READ_ROOTS_KNOB: &str = "CRAFT_AGENT_READ_ROOTS";

const MAX_KNOB_BYTES: usize = 4096;
const MAX_READ_ROOTS: usize = 4;

/// Knob value -> extra read roots. None or empty turns the feature
/// off; an oversized value grants nothing (fail closed). Entries are
/// trimmed; empty and non-absolute entries are dropped; at most
/// MAX_READ_ROOTS survive, in declaration order. A root that does not
/// exist is tolerated — reads inside it return `NotFound`.
pub fn parse_read_roots(knob: Option<&str>) -> Vec<PathBuf> {
    let Some(value) = knob else {
        return Vec::new();
    };
    if value.trim().is_empty() || value.len() > MAX_KNOB_BYTES {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter(|entry| Path::new(entry).is_absolute())
        .map(PathBuf::from)
        .take(MAX_READ_ROOTS)
        .collect()
}

/// Resolve a tool-supplied path against the workspace root, rejecting
/// lexical escapes, symlink escapes, and dangling symlinks. The
/// returned path is contained in the root lexically, and every symlink
/// component on it resolves inside the root to a non-sensitive target.
pub fn resolve_within(
    tool: &str,
    root: &Path,
    input: &str,
    max_path_bytes: usize,
) -> Result<PathBuf, ToolError> {
    let canon_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let folded = fold_within(tool, &canon_root, input, max_path_bytes)?;
    reject_symlink_escapes(&canon_root, &folded, input)?;
    Ok(folded)
}

/// Resolve a READ path: a relative input folds against the workspace
/// root only (`resolve_within`, verbatim); an absolute input may land
/// in the workspace or a configured read root, tried in that order.
/// The first containing root wins; its symlink verdict is definitive,
/// so a link may not stitch roots; a probe outside every root stays
/// the run-fatal `EscapesWorkspace` denial.
pub fn resolve_read(
    tool: &str,
    workspace: &Path,
    read_roots: &[PathBuf],
    input: &str,
    max_path_bytes: usize,
) -> Result<PathBuf, ToolError> {
    if !Path::new(input).is_absolute() {
        return resolve_within(tool, workspace, input, max_path_bytes);
    }
    let candidates = std::iter::once(workspace).chain(read_roots.iter().map(PathBuf::as_path));
    for root in candidates {
        let canon_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let folded = match fold_within(tool, &canon_root, input, max_path_bytes) {
            Ok(folded) => folded,
            Err(ToolError::EscapesWorkspace { .. }) => continue,
            Err(other) => return Err(other),
        };
        reject_symlink_escapes(&canon_root, &folded, input)?;
        return Ok(folded);
    }
    Err(escapes(input))
}

/// Fold the input lexically: `.` drops, `..` pops, and an absolute
/// input must still land inside the root.
fn fold_within(
    tool: &str,
    root: &Path,
    input: &str,
    max_path_bytes: usize,
) -> Result<PathBuf, ToolError> {
    if input.is_empty() {
        return Err(arg_err(tool, "path must not be empty".to_string()));
    }
    if input.len() > max_path_bytes {
        return Err(arg_err(
            tool,
            format!("path exceeds {max_path_bytes} bytes"),
        ));
    }
    let input_path = Path::new(input);
    let absolute = input_path.is_absolute();
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for comp in input_path.components() {
        match comp {
            Component::Normal(c) => parts.push(c.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(escapes(input));
                }
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    let mut folded = if absolute {
        PathBuf::from("/")
    } else {
        root.to_path_buf()
    };
    for part in &parts {
        folded.push(part);
    }
    if !folded.starts_with(root) {
        return Err(escapes(input));
    }
    Ok(folded)
}

/// Walk the folded path component by component. Every symlink must
/// canonicalize inside the root to a non-sensitive target; a dangling
/// symlink is denied outright, because writing through one would
/// create its target outside the root.
fn reject_symlink_escapes(canon_root: &Path, folded: &Path, input: &str) -> Result<(), ToolError> {
    let rel = match folded.strip_prefix(canon_root) {
        Ok(rel) => rel,
        Err(_) => return Err(escapes(input)),
    };
    let mut resolved = canon_root.to_path_buf();
    for comp in rel.components() {
        resolved.push(comp);
        let Ok(meta) = std::fs::symlink_metadata(&resolved) else {
            continue;
        };
        if !meta.file_type().is_symlink() {
            continue;
        }
        let Ok(target) = std::fs::canonicalize(&resolved) else {
            return Err(escapes(input));
        };
        if !target.starts_with(canon_root) {
            return Err(escapes(input));
        }
        if let Some(reason) = sensitive_reason(&target) {
            return Err(ToolError::SensitivePath {
                path: input.to_string(),
                reason,
            });
        }
        resolved = target;
    }
    Ok(())
}

/// The reason a resolved path is sensitive, if it is. Checked per call
/// against every path component.
pub fn sensitive_reason(path: &Path) -> Option<&'static str> {
    for comp in path.components() {
        let name = comp.as_os_str().to_string_lossy();
        let lower = name.to_lowercase();
        if lower == ".aws" || lower == ".gcp" {
            return Some("cloud credentials directory");
        }
        if lower.starts_with(".env") && !is_exempt_env(&name) {
            return Some("environment file");
        }
        if is_key_or_token(&lower) && !name.ends_with(".pub") {
            return Some("key or token file");
        }
        if lower.starts_with("credentials") {
            return Some("credentials file");
        }
    }
    None
}

fn is_exempt_env(name: &str) -> bool {
    matches!(name, ".env.example" | ".env.sample" | ".env.template")
}

fn is_key_or_token(lower: &str) -> bool {
    lower.starts_with("id_rsa")
        || lower.starts_with("id_dsa")
        || lower.starts_with("id_ed25519")
        || lower.starts_with("id_ecdsa")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".netrc")
        || lower.ends_with(".npmrc")
}

#[cfg(test)]
mod tests {
    use super::{parse_read_roots, sensitive_reason};
    use std::path::{Path, PathBuf};

    fn reason(p: &str) -> Option<&'static str> {
        sensitive_reason(Path::new(p))
    }

    fn roots(raw: &str) -> Vec<PathBuf> {
        parse_read_roots(Some(raw))
    }

    #[test]
    fn parse_read_roots_rows() {
        assert!(parse_read_roots(None).is_empty());
        assert!(roots("").is_empty());
        assert!(roots("   ").is_empty());
        assert!(roots(",,").is_empty());
        assert_eq!(
            roots(" /a , /b ,, sub , "),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert_eq!(roots("/r1,/r2,/r3").len(), 3);
        assert_eq!(
            roots("/r1,/r2,/r3"),
            vec![
                PathBuf::from("/r1"),
                PathBuf::from("/r2"),
                PathBuf::from("/r3"),
            ]
        );
        assert_eq!(
            roots("/r1,/r2,/r3,/r4,/r5"),
            vec![
                PathBuf::from("/r1"),
                PathBuf::from("/r2"),
                PathBuf::from("/r3"),
                PathBuf::from("/r4"),
            ]
        );
        assert!(roots(&format!("/{}", "x".repeat(4096))).is_empty());
    }

    #[test]
    fn bans_match_and_exemptions_hold() {
        assert!(reason("/ws/.env").is_some());
        assert!(reason("/ws/.env.production").is_some());
        assert!(reason("/ws/sub/id_rsa").is_some());
        assert!(reason("/ws/sub/id_dsa").is_some());
        assert!(reason("/ws/server.pem").is_some());
        assert!(reason("/ws/tls/site.key").is_some());
        assert!(reason("/ws/.netrc").is_some());
        assert!(reason("/ws/.npmrc").is_some());
        assert!(reason("/ws/.aws/config").is_some());
        assert!(reason("/ws/.gcp/creds").is_some());
        assert!(reason("/ws/credentials.json").is_some());
        assert_eq!(reason("/ws/.env.example"), None);
        assert_eq!(reason("/ws/id_rsa.pub"), None);
        assert_eq!(reason("/ws/id_dsa.pub"), None);
        assert_eq!(reason("/ws/notes.txt"), None);
    }
}
