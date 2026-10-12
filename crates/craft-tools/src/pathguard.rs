//! Workspace path containment and the sensitive-path predicate.
//!
//! Sensitive paths are denied on every call: the guard is a pure
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
    use super::sensitive_reason;
    use std::path::Path;

    fn reason(p: &str) -> Option<&'static str> {
        sensitive_reason(Path::new(p))
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
