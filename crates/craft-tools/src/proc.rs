//! The child-process engine behind bash and gh: piped capture bounded by
//! an output cap, a per-run timeout, and the kill sequence SIGTERM →
//! grace window → SIGKILL. The child owns a fresh process group so the
//! kill sequence takes its background jobs with it. The loop polls for
//! child exit, so a script that leaves a pipe-holding grandchild still
//! finishes at the child's own exit rather than the timeout. Chunks
//! stream as progress while the child runs; the exit event carries the
//! bounded accumulated output.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::stream::Stream;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
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
/// How often the loop polls the child for exit while pipes stay open.
const EXIT_POLL: Duration = Duration::from_millis(100);
/// How long pipes are drained once the child has exited before giving up
/// on grandchildren that still hold them.
const PIPE_DRAIN: Duration = Duration::from_millis(200);

/// Spawn the child and return its event stream. The pump task drives the
/// child to completion; dropping the stream kills it.
pub fn spawn(
    spec: ProcSpec,
) -> Result<impl Stream<Item = Result<ProcEvent, ToolError>> + Send, ToolError> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.argv)
        .current_dir(&spec.cwd)
        .process_group(0)
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
    drain_until: Option<Instant>,
    pgid: Option<i32>,
    aborted: bool,
    done: bool,
}

impl Pump {
    fn new(spec: &ProcSpec, pgid: Option<i32>) -> Self {
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
            drain_until: None,
            pgid,
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
    let pgid = child.id().and_then(|id| i32::try_from(id).ok());
    let mut state = Pump::new(&spec, pgid);
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

/// One select round: poll the child, advance the kill-state machine,
/// then drain whichever pipe has data.
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
    if state.drain_until.is_none() {
        // The kill ladder runs only while the child is alive; once it has
        // exited, the pipes get a short drain before giving up on
        // grandchildren that still hold them.
        if child.try_wait().ok().flatten().is_some() {
            state.drain_until = Some(now + PIPE_DRAIN);
        } else if advance_kill(state, child, now, spec) {
            return;
        }
    }
    if let Some(until) = state.drain_until
        && now >= until
    {
        // Leftovers of a timed-out script die with it; a script that
        // finished on its own keeps its background jobs.
        if state.grace_at.is_some() {
            kill_group(state, child);
        }
        state.done = true;
        return;
    }
    let phase = match (state.grace_at, state.final_at) {
        (Some(_), Some(final_at)) => final_at,
        (Some(grace), None) => grace,
        (None, _) => state.deadline,
    };
    let mut wake = phase;
    if let Some(drain) = state.drain_until {
        wake = wake.min(drain);
    }
    let wake = wake.min(now + EXIT_POLL);
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

/// The kill ladder SIGTERM → grace → SIGKILL → final wait; runs only
/// while the child is alive. Returns true when the final wait expires.
fn advance_kill(state: &mut Pump, child: &mut Child, now: Instant, spec: &ProcSpec) -> bool {
    if state.grace_at.is_none() && now >= state.deadline {
        send_sigterm(state, child);
        state.grace_at = Some(now + spec.term_grace);
    }
    if let Some(grace) = state.grace_at
        && now >= grace
        && state.final_at.is_none()
    {
        kill_group(state, child);
        state.final_at = Some(now + spec.term_grace * FINAL_GRACE_MULT);
    }
    if let Some(final_at) = state.final_at
        && now >= final_at
    {
        state.done = true;
        return true;
    }
    false
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
        Ok(0) | Err(_) => {
            mark_done(state, tag);
            return;
        }
        Ok(n) => n,
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
            kill_group(state, child);
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
        kill_group(state, child);
    }
    let status = match tokio::time::timeout(spec.term_grace, child.wait()).await {
        Ok(Ok(status)) => Some(status),
        _ => {
            kill_group(state, child);
            child.wait().await.ok()
        }
    };
    let code = status.as_ref().and_then(|s| s.code());
    let (stdout, stdout_truncated) = util::bounded_lossy(&state.out_acc, spec.out_cap);
    let (stderr, stderr_truncated) = util::bounded_lossy(&state.err_acc, spec.out_cap);
    ProcExit {
        code,
        timed_out: state.grace_at.is_some(),
        stdout,
        stderr,
        stdout_truncated: state.out_truncated || stdout_truncated,
        stderr_truncated: state.err_truncated || stderr_truncated,
    }
}

/// SIGTERM the whole process group; a vanished group (ESRCH) is not an
/// error.
fn send_sigterm(state: &Pump, child: &mut Child) {
    signal_group(state, Signal::SIGTERM);
    if let Some(id) = child.id()
        && let Ok(pid) = i32::try_from(id)
    {
        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
    }
}

/// SIGKILL the whole group plus the child itself, for a spent grace
/// window, a dead receiver, or a timed-out script's leftover jobs.
fn kill_group(state: &Pump, child: &mut Child) {
    signal_group(state, Signal::SIGKILL);
    let _ = child.start_kill();
}

fn signal_group(state: &Pump, sig: Signal) {
    if let Some(pgid) = state.pgid {
        let _ = killpg(Pid::from_raw(pgid), sig);
    }
}
