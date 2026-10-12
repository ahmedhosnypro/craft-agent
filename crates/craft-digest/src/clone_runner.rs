//! Shallow clones of the suite repositories into a scratch dir, run
//! directly via std::process::Command; the program is injectable so the
//! corpora drive fixture scripts.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::bundle::{Bundle, file_slug};
use crate::error::{DigestError, bounded};

/// Wall-clock bound for one shallow clone; expiry becomes an error row.
pub const CLONE_DEADLINE: Duration = Duration::from_secs(600);
const WAIT_POLL: Duration = Duration::from_millis(50);

pub struct CloneRunner {
    program: PathBuf,
    deadline: Duration,
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
            deadline: CLONE_DEADLINE,
        }
    }

    /// Tests point this at a fake git script.
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        CloneRunner {
            program: program.into(),
            deadline: CLONE_DEADLINE,
        }
    }

    /// Tests shorten the bound to exercise expiry.
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
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
    let mut command = Command::new(&runner.program);
    command
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(&url)
        .arg(&dir);
    match run_with_deadline(&mut command, runner.deadline) {
        Ok(BoundedRun::Completed { status, .. }) if status.success() => {}
        Ok(BoundedRun::Completed { stderr, .. }) => {
            let stderr = String::from_utf8_lossy(&stderr);
            bundle.push_error(repo, "clone", &stderr);
        }
        Ok(BoundedRun::Expired) => {
            let message = format!("clone exceeded its {:?} deadline", runner.deadline);
            bundle.push_error(repo, "clone", &message);
        }
        Err(err) => bundle.push_error(repo, "clone", &err.to_string()),
    }
}

/// A child run to completion, or killed at its deadline.
pub(crate) enum BoundedRun {
    Completed {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    Expired,
}

/// Run a child to completion or kill it at `deadline`; the pipes are
/// drained concurrently so a full output page cannot false-expire the wait.
pub(crate) fn run_with_deadline(
    command: &mut Command,
    deadline: Duration,
) -> std::io::Result<BoundedRun> {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let readers = drain(&mut child);
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let (stdout, stderr) = join_readers(readers);
                return Ok(BoundedRun::Completed {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if start.elapsed() < deadline => std::thread::sleep(WAIT_POLL),
            Ok(None) => {
                kill_and_reap(&mut child);
                let _ = join_readers(readers);
                return Ok(BoundedRun::Expired);
            }
            Err(err) => {
                kill_and_reap(&mut child);
                let _ = join_readers(readers);
                return Err(err);
            }
        }
    }
}

fn drain(child: &mut Child) -> (JoinHandle<Vec<u8>>, JoinHandle<Vec<u8>>) {
    (
        spawn_reader(child.stdout.take()),
        spawn_reader(child.stderr.take()),
    )
}

fn spawn_reader<R: Read + Send + 'static>(handle: Option<R>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut handle) = handle {
            let _ = handle.read_to_end(&mut buffer);
        }
        buffer
    })
}

fn join_readers(readers: (JoinHandle<Vec<u8>>, JoinHandle<Vec<u8>>)) -> (Vec<u8>, Vec<u8>) {
    let (out, err) = readers;
    (
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    )
}

fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
