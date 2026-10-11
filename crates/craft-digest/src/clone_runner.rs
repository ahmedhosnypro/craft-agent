//! Shallow clones of the suite repositories into a scratch dir, run
//! directly via std::process::Command; the program is injectable so the
//! corpora drive fixture scripts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bundle::{Bundle, file_slug};
use crate::error::{DigestError, bounded};

pub struct CloneRunner {
    program: PathBuf,
}

impl Default for CloneRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CloneRunner {
    pub fn new() -> Self {
        CloneRunner {
            program: PathBuf::from("git"),
        }
    }

    /// Tests point this at a fake git script.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        CloneRunner {
            program: program.into(),
        }
    }
}

/// Shallow-clone every repo under `<scratch_root>/<repo>`; a failing repo
/// becomes an error row and the run continues.
pub fn clone_all(
    runner: &CloneRunner,
    org: &str,
    repos: &[String],
    bundle: &mut Bundle,
    scratch_root: &Path,
) -> Result<(), DigestError> {
    fs::create_dir_all(scratch_root).map_err(|err| DigestError::Io(bounded(&err.to_string())))?;
    for repo in repos {
        clone_repo(runner, org, repo, bundle, scratch_root);
    }
    Ok(())
}

fn clone_repo(
    runner: &CloneRunner,
    org: &str,
    repo: &str,
    bundle: &mut Bundle,
    scratch_root: &Path,
) {
    let dir = scratch_root.join(file_slug(repo));
    let url = format!("https://github.com/{org}/{repo}.git");
    let output = Command::new(&runner.program)
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(&url)
        .arg(&dir)
        .output();
    match output {
        Ok(status) if status.status.success() => {}
        Ok(status) => {
            let stderr = String::from_utf8_lossy(&status.stderr);
            bundle.push_error(repo, "clone", &stderr);
        }
        Err(err) => bundle.push_error(repo, "clone", &err.to_string()),
    }
}
