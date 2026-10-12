//! The gh write-restriction gate: `CRAFT_AGENT_WRITE_REPO` declares the
//! single repository gh commands may write to. Write-class subcommands
//! must carry an explicit -R/--repo target (or a repos/<owner>/<repo>
//! API path) naming it; reads always pass, whatever repo they name. A
//! non-empty knob that is not `owner/name` denies writes outright.
//! `gh auth setup-git` is denied while the knob is set: the git
//! credential helper is host-wide, so allowing it would let plain
//! `git push` reach the token for any repository.

use crate::error::ToolError;

pub const KNOB: &str = "CRAFT_AGENT_WRITE_REPO";

const MAX_KNOB_BYTES: usize = 128;
const API_WRITE_METHODS: &[&str] = &["POST", "PUT", "PATCH", "DELETE"];

/// The repository declared by the knob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteRepo {
    /// A valid lowercased `owner/name`.
    Target(String),
    /// A non-empty but malformed knob: reads pass, writes and
    /// `auth setup-git` deny.
    Malformed,
}

/// Parse the knob: None means the gate is off, Malformed means the value
/// was set but is not a bare `owner/name`.
pub fn write_repo(knob: Option<&str>) -> Option<WriteRepo> {
    let value = knob?;
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_KNOB_BYTES {
        return none_or_malformed(trimmed);
    }
    match trimmed.split_once('/') {
        Some((owner, name))
            if !owner.is_empty()
                && !name.is_empty()
                && !name.contains('/')
                && !owner.chars().any(char::is_whitespace)
                && !name.chars().any(char::is_whitespace) =>
        {
            Some(WriteRepo::Target(format!(
                "{}/{}",
                owner.to_lowercase(),
                name.to_lowercase()
            )))
        }
        _ => Some(WriteRepo::Malformed),
    }
}

fn none_or_malformed(trimmed: &str) -> Option<WriteRepo> {
    if trimmed.is_empty() {
        None
    } else {
        Some(WriteRepo::Malformed)
    }
}

/// Gate one gh argv under the declared write repo.
pub fn check(argv: &[String], repo: &WriteRepo) -> Result<(), ToolError> {
    let valid = match repo {
        WriteRepo::Target(name) => Some(name.as_str()),
        WriteRepo::Malformed => None,
    };
    match classify(argv) {
        Verb::Read => Ok(()),
        Verb::SetupGit => Err(ToolError::GhSetupGitDenied),
        Verb::Write => {
            if valid.is_none() {
                return Err(ToolError::GhWriteRepoMalformed);
            }
            let mut targets = repo_targets(argv);
            if argv.first().is_some_and(|a| a == "repo")
                && let Some(positional) = argv.get(2)
                && let Some(target) = repo_target(positional)
            {
                targets.push(target);
            }
            if targets.is_empty() {
                return Err(ToolError::GhWriteUntargeted);
            }
            for target in targets {
                if Some(target.as_str()) != valid {
                    return Err(ToolError::GhWriteDenied { target });
                }
            }
            Ok(())
        }
    }
}

/// A bare `owner/name` target, lowercased; anything else is not one.
fn repo_target(token: &str) -> Option<String> {
    let (owner, name) = token.split_once('/')?;
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || owner.chars().any(char::is_whitespace)
        || name.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(format!("{}/{}", owner.to_lowercase(), name.to_lowercase()))
}

enum Verb {
    Read,
    Write,
    SetupGit,
}

/// Classify a gh argv. Known repo-scoped families are writes except for
/// their documented read subcommands; unrecognized subcommands inside
/// those families default to write, and unrecognized first-level
/// commands (browse, search, ...) stay passable.
fn classify(argv: &[String]) -> Verb {
    let Some(first) = argv.first() else {
        return Verb::Read;
    };
    match first.as_str() {
        "pr" => sub(argv, &["list", "view", "diff", "status", "checks"]),
        "issue" => sub(argv, &["list", "view", "status"]),
        "repo" => sub(argv, &["list", "view", "clone"]),
        "release" => sub(argv, &["list", "view", "download"]),
        "secret" => sub(argv, &["list"]),
        "variable" => sub(argv, &["list"]),
        "label" => sub(argv, &["list"]),
        "workflow" => sub(argv, &["list", "view"]),
        "run" => sub(argv, &["list", "view", "watch"]),
        "push" => Verb::Write,
        "api" => api_verb(argv),
        "auth" if argv.get(1).is_some_and(|s| s == "setup-git") => Verb::SetupGit,
        _ => Verb::Read,
    }
}

fn sub(argv: &[String], reads: &[&str]) -> Verb {
    match argv.get(1).map(String::as_str) {
        Some(name) if reads.contains(&name) => Verb::Read,
        _ => Verb::Write,
    }
}

/// `gh api` is a write when the method flag names one of the mutating
/// methods or the argv carries request-body flags.
fn api_verb(argv: &[String]) -> Verb {
    const BODY_FLAGS: &[&str] = &["-f", "--field", "-F", "--raw-field", "--input"];
    let mut write = false;
    let mut idx = 1;
    while idx < argv.len() {
        let arg = argv[idx].as_str();
        let method = if arg == "-X" || arg == "--method" {
            idx += 1;
            match argv.get(idx) {
                Some(value) => value.clone(),
                None => {
                    write = true;
                    String::new()
                }
            }
        } else if let Some(value) = arg.strip_prefix("--method=") {
            value.to_string()
        } else if let Some(value) = arg.strip_prefix("-X") {
            value.to_string()
        } else {
            String::new()
        };
        if API_WRITE_METHODS.contains(&method.to_uppercase().as_str()) {
            write = true;
        }
        if BODY_FLAGS.contains(&arg) || arg.starts_with("--field=") {
            write = true;
        }
        idx += 1;
    }
    if write { Verb::Write } else { Verb::Read }
}

/// Every explicit repo target in the argv, lowercased: -R/--repo values
/// (separate, `=`, or attached) and repos/<owner>/<repo> API paths.
fn repo_targets(argv: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < argv.len() {
        let arg = argv[idx].as_str();
        if arg == "-R" || arg == "--repo" {
            idx += 1;
            if let Some(value) = argv.get(idx) {
                out.push(value.to_lowercase());
            }
        } else if let Some(value) = arg
            .strip_prefix("--repo=")
            .or_else(|| arg.strip_prefix("-R="))
        {
            out.push(value.to_lowercase());
        } else if let Some(path) = repos_path(arg)
            && let Some(repo) = repo_from_path(path)
        {
            out.push(repo);
        }
        idx += 1;
    }
    out
}

fn repos_path(arg: &str) -> Option<&str> {
    let at = arg.find("repos/")?;
    Some(&arg[at + "repos/".len()..])
}

fn repo_from_path(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some(format!("{}/{}", owner.to_lowercase(), name.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::{KNOB, WriteRepo, check, repo_from_path, write_repo};

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn knob_parsing() {
        assert!(write_repo(None).is_none());
        assert!(write_repo(Some("")).is_none());
        assert!(write_repo(Some("  ")).is_none());
        assert_eq!(
            write_repo(Some("Acme/Widgets")),
            Some(WriteRepo::Target(String::from("acme/widgets")))
        );
        for bad in ["acme", "a/b/c", " /widgets", "owner/na me"] {
            assert_eq!(write_repo(Some(bad)), Some(WriteRepo::Malformed), "{bad}");
        }
    }

    #[test]
    fn api_path_shapes() {
        assert_eq!(
            repo_from_path("acme/widgets/pulls").as_deref(),
            Some("acme/widgets")
        );
        assert!(repo_from_path("acme").is_none());
        assert!(repo_from_path("").is_none());
    }

    #[test]
    fn gate_decisions() {
        let repo = WriteRepo::Target(String::from("acme/widgets"));
        assert!(check(&argv(&["pr", "view", "-R", "x/y"]), &repo).is_ok());
        assert!(check(&argv(&["pr", "create", "-R", "acme/widgets"]), &repo).is_ok());
        assert!(check(&argv(&["pr", "create", "-R", "x/y"]), &repo).is_err());
        assert!(check(&argv(&["pr", "create"]), &repo).is_err());
        assert!(check(&argv(&["auth", "setup-git"]), &repo).is_err());
        assert!(KNOB == "CRAFT_AGENT_WRITE_REPO");
    }
}
