//! The child-process engine behind bash and gh: piped capture bounded by
//! an output cap, a per-run timeout, and the kill sequence SIGTERM →
//! grace window → SIGKILL. Chunks stream as progress while the child
//! runs; the exit event carries the bounded accumulated output.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::stream::Stream;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use crate::error::ToolError;
use crate::util;

/// Everything needed to run one child.
#[derive(Debug, Clone)]
pub struct ProcSpec {
    pub program: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub term_grace: Duration,
    pub out_cap: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamTag {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcEvent {
    Chunk { stream: StreamTag, text: String },
    Exit(ProcExit),
}

const READ_BUF: usize = 8 * 1024;
const FINAL_GRACE_MULT: u32 = 2;

/// Spawn the child and return its event stream. The pump task drives the
/// child to completion; dropping the stream kills it.
pub fn spawn(
    spec: ProcSpec,
) -> Result<impl Stream<Item = Result<ProcEvent, ToolError>> + Send, ToolError> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.argv)
        .current_dir(&spec.cwd)
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| ToolError::Spawn {
        program: spec.program.clone(),
        msg: e.to_string(),
    })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (stdout, stderr) = match (stdout, stderr) {
        (Some(o), Some(e)) => (o, e),
        _ => {
            let _ = child.start_kill();
            return Err(ToolError::Spawn {
                program: spec.program,
                msg: "child opened without pipes".to_string(),
            });
        }
    };
    let (tx, rx) = mpsc::unbounded();
    tokio::spawn(pump(child, stdout, stderr, tx, spec));
    Ok(rx)
}

struct Pump {
    out_acc: Vec<u8>,
    err_acc: Vec<u8>,
    out_truncated: bool,
    err_truncated: bool,
    out_done: bool,
    err_done: bool,
    deadline: Instant,
    grace_at: Option<Instant>,
    final_at: Option<Instant>,
    aborted: bool,
    done: bool,
}

impl Pump {
    fn new(spec: &ProcSpec) -> Self {
        Self {
            out_acc: Vec::new(),
            err_acc: Vec::new(),
            out_truncated: false,
            err_truncated: false,
            out_done: false,
            err_done: false,
            deadline: Instant::now() + spec.timeout,
            grace_at: None,
            final_at: None,
            aborted: false,
            done: false,
        }
    }
}

async fn pump(
    mut child: Child,
    mut out: ChildStdout,
    mut err: ChildStderr,
    tx: mpsc::UnboundedSender<Result<ProcEvent, ToolError>>,
    spec: ProcSpec,
) {
    let mut state = Pump::new(&spec);
    let mut out_buf = [0u8; READ_BUF];
    let mut err_buf = [0u8; READ_BUF];
    while !state.done {
        let mut pipes = Pipes {
            out: &mut out,
            err: &mut err,
            out_buf: &mut out_buf,
            err_buf: &mut err_buf,
        };
        tick(&mut state, &mut pipes, &mut child, &tx, &spec).await;
    }
    let exit = reap(&mut state, &mut child, &spec).await;
    let _ = tx.unbounded_send(Ok(ProcEvent::Exit(exit)));
}

/// One select round: advance the kill-state machine, then drain whichever
/// pipe has data.
struct Pipes<'a> {
    out: &'a mut ChildStdout,
    err: &'a mut ChildStderr,
    out_buf: &'a mut [u8; READ_BUF],
    err_buf: &'a mut [u8; READ_BUF],
}

async fn tick(
    state: &mut Pump,
    pipes: &mut Pipes<'_>,
    child: &mut Child,
    tx: &mpsc::UnboundedSender<Result<ProcEvent, ToolError>>,
    spec: &ProcSpec,
) {
    if state.out_done && state.err_done {
        state.done = true;
        return;
    }
    let now = Instant::now();
    if state.grace_at.is_none() && now >= state.deadline {
        send_sigterm(child);
        state.grace_at = Some(now + spec.term_grace);
    }
    if let Some(grace) = state.grace_at
        && now >= grace
        && state.final_at.is_none()
    {
        kill_child(child);
        state.final_at = Some(now + spec.term_grace * FINAL_GRACE_MULT);
    }
    if let Some(final_at) = state.final_at
        && now >= final_at
    {
        state.done = true;
        return;
    }
    let wake = state
        .grace_at
        .or(state.final_at)
        .unwrap_or(state.deadline)
        .min(state.deadline);
    let out_pending = !state.out_done;
    let err_pending = !state.err_done;
    tokio::select! {
        read = pipes.out.read(pipes.out_buf), if out_pending => {
            let buf = &pipes.out_buf[..];
            absorb(state, StreamTag::Stdout, read, spec.out_cap, buf, tx, child);
        }
        read = pipes.err.read(pipes.err_buf), if err_pending => {
            let buf = &pipes.err_buf[..];
            absorb(state, StreamTag::Stderr, read, spec.out_cap, buf, tx, child);
        }
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(wake)) => {}
    }
}

/// Record one pipe read: EOF closes the pipe, data fills the bounded
/// accumulator and streams as a chunk until the cap is hit, a dead
/// receiver aborts and kills the child.
fn absorb(
    state: &mut Pump,
    tag: StreamTag,
    read: std::io::Result<usize>,
    cap: usize,
    buf: &[u8],
    tx: &mpsc::UnboundedSender<Result<ProcEvent, ToolError>>,
    child: &mut Child,
) {
    let n = match read {
        Ok(0) => {
            mark_done(state, tag);
            return;
        }
        Ok(n) => n,
        Err(_) => {
            mark_done(state, tag);
            return;
        }
    };
    let (out_acc, trunc_flag) = split_acc(state, tag);
    let take = cap.saturating_sub(out_acc.len()).min(n);
    let room = take > 0;
    if room {
        let data = &buf[..take];
        let text = String::from_utf8_lossy(data).into_owned();
        out_acc.extend_from_slice(data);
        if take < n {
            *trunc_flag = true;
        }
        let sent = tx.unbounded_send(Ok(ProcEvent::Chunk { stream: tag, text }));
        if sent.is_err() {
            state.aborted = true;
            state.done = true;
            kill_child(child);
        }
    } else if n > take {
        *trunc_flag = true;
    }
}

fn mark_done(state: &mut Pump, tag: StreamTag) {
    match tag {
        StreamTag::Stdout => state.out_done = true,
        StreamTag::Stderr => state.err_done = true,
    }
}

fn split_acc(state: &mut Pump, tag: StreamTag) -> (&mut Vec<u8>, &mut bool) {
    match tag {
        StreamTag::Stdout => (&mut state.out_acc, &mut state.out_truncated),
        StreamTag::Stderr => (&mut state.err_acc, &mut state.err_truncated),
    }
}

/// Reap the child, bounded in time: a normal wait inside the grace
/// window, then SIGKILL plus a final wait. Always consumes the child.
async fn reap(state: &mut Pump, child: &mut Child, spec: &ProcSpec) -> ProcExit {
    if state.aborted {
        kill_child(child);
    }
    let status = match tokio::time::timeout(spec.term_grace, child.wait()).await {
        Ok(Ok(status)) => Some(status),
        _ => {
            kill_child(child);
            child.wait().await.ok()
        }
    };
    let (code, signal) = match &status {
        Some(status) => (
            status.code(),
            std::os::unix::process::ExitStatusExt::signal(status),
        ),
        None => (None, None),
    };
    let (stdout, stdout_truncated) = util::bounded_lossy(&state.out_acc, spec.out_cap);
    let (stderr, stderr_truncated) = util::bounded_lossy(&state.err_acc, spec.out_cap);
    ProcExit {
        code,
        signal,
        timed_out: state.grace_at.is_some(),
        stdout,
        stderr,
        stdout_truncated: state.out_truncated || stdout_truncated,
        stderr_truncated: state.err_truncated || stderr_truncated,
    }
}

/// Deliver SIGTERM; a vanished child (ESRCH) is not an error.
fn send_sigterm(child: &mut Child) {
    if let Some(id) = child.id()
        && let Ok(pid) = i32::try_from(id)
    {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
}

/// SIGKILL, for when the grace window is spent or the stream is gone.
fn kill_child(child: &mut Child) {
    let _ = child.start_kill();
}
