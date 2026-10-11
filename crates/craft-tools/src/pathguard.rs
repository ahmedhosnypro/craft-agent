//! Workspace path containment and the sensitive-path predicate.
//!
//! Sensitive paths are denied on every call: the guard is a pure
//! function of the resolved path, so nothing is cached across calls.
//! Ban patterns match case-insensitively; the public exemptions
//! (`.env.example`/`.env.sample`/`.env.template`, `*.pub` keys) require
//! the exact documented spellings.

use std::path::{Component, Path, PathBuf};

use crate::error::ToolError;

fn arg_err(tool: &str, msg: String) -> ToolError {
    ToolError::InvalidArgs {
        tool: tool.to_string(),
        msg,
    }
}

/// Resolve a tool-supplied path against the workspace root, rejecting
/// lexical escapes and symlink escapes. Returns an absolute path that is
/// contained in the root by both lexical folding and canonicalization of
/// the deepest existing ancestor.
pub fn resolve_within(
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
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let input_path = Path::new(input);
    let absolute = input_path.is_absolute();
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for comp in input_path.components() {
        match comp {
            Component::Normal(c) => parts.push(c.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(ToolError::EscapesWorkspace {
                        path: input.to_string(),
                    });
                }
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    let folded = if absolute {
        let mut path = PathBuf::from("/");
        for part in &parts {
            path.push(part);
        }
        path
    } else {
        let mut path = root.clone();
        for part in &parts {
            path.push(part);
        }
        path
    };
    if !folded.starts_with(&root) {
        return Err(ToolError::EscapesWorkspace {
            path: input.to_string(),
        });
    }
    let canon_root = std::fs::canonicalize(&root).unwrap_or(root);
    let mut existing = folded.clone();
    while !existing.exists() {
        if !existing.pop() {
            break;
        }
    }
    if let Ok(canon) = std::fs::canonicalize(&existing)
        && !canon.starts_with(&canon_root)
    {
        return Err(ToolError::EscapesWorkspace {
            path: input.to_string(),
        });
    }
    Ok(folded)
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
        if is_private_key(&lower) && !name.ends_with(".pub") {
            return Some("private key file");
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

fn is_private_key(lower: &str) -> bool {
    lower.starts_with("id_rsa") || lower.starts_with("id_ed25519") || lower.starts_with("id_ecdsa")
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
        assert!(reason("/ws/.aws/config").is_some());
        assert!(reason("/ws/.gcp/creds").is_some());
        assert!(reason("/ws/credentials.json").is_some());
        assert_eq!(reason("/ws/.env.example"), None);
        assert_eq!(reason("/ws/id_rsa.pub"), None);
        assert_eq!(reason("/ws/notes.txt"), None);
    }
}
