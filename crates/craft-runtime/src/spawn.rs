//! Spawn-tool machinery: argument validation with every input
//! hostile (bounded numbers, bounded prompts, exact-placeholder
//! template fill, duplicate detection), the tracked child future
//! (wall-clock arming, settle-on-drop backstop), and the one stream
//! driver both tools ride — launch up to the concurrency cap, queue
//! the rest FIFO, settle each completion exactly once, and render
//! the single envelope or the batch aggregate.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use craft_core::subagent::handoff::{Handoff, render_batch};
use futures::FutureExt;
use futures::stream::{FuturesUnordered, StreamExt};

use craft_tools::event::ToolStream;
use craft_tools::{ToolError, ToolEvent, ToolProgress, ToolResult};

use crate::child::{ChildFuture, ChildKnobs, ChildRun, prepare_child};
use crate::envelope::ChildReport;
use crate::spawn_args::SpawnRequest;
use crate::subagent::{SubagentHub, SubagentWiring};
use crate::subagent_admit::{AdmitBatch, PoolFull};
use craft_core::subagent::SUBAGENT_OVERLAY;
/// One child future with wall-clock arming and the settle-on-drop
/// backstop: a dropped child settles exactly once — completed,
/// timed out, or (when the parent itself died mid-drive) a
/// `spawn_error` report built from the noted spend.
pub(crate) struct TrackedChild {
    hub: Arc<SubagentHub>,
    id: String,
    timeout_secs: Option<u64>,
    launched: bool,
    done: bool,
    inner: Pin<Box<dyn Future<Output = Result<ChildReport, ()>> + Send>>,
}

impl TrackedChild {
    pub(crate) fn new(
        hub: Arc<SubagentHub>,
        id: String,
        future: ChildFuture,
        timeout_secs: Option<u64>,
    ) -> Self {
        let inner = match timeout_secs {
            Some(secs) => tokio::time::timeout(Duration::from_secs(secs), future)
                .map(|result| result.map_err(|_| ()))
                .boxed(),
            None => future.map(Ok).boxed(),
        };
        Self {
            hub,
            id,
            timeout_secs,
            launched: false,
            done: false,
            inner,
        }
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// The wall clock armed on this child, for the launch event.
    pub(crate) fn timeout_secs(&self) -> Option<u64> {
        self.timeout_secs
    }

    /// The hub this child settles against, for a detached pool task.
    pub(crate) fn hub(&self) -> Arc<SubagentHub> {
        self.hub.clone()
    }

    pub(crate) fn mark_launched(&mut self) {
        self.launched = true;
    }
}

impl Future for TrackedChild {
    type Output = (String, ChildReport);

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match this.inner.as_mut().poll(cx) {
            Poll::Ready(Ok(report)) => {
                this.done = true;
                Poll::Ready((this.id.clone(), report))
            }
            Poll::Ready(Err(_elapsed)) => {
                this.done = true;
                let secs = this.timeout_secs.unwrap_or(0);
                let report = this.hub.timed_out_report(&this.id, secs);
                Poll::Ready((this.id.clone(), report))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for TrackedChild {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if self.launched {
            self.hub
                .settle(&self.id, &self.hub.dropped_report(&self.id));
        } else {
            self.hub.release(&self.id);
        }
    }
}

/// Prepares one child run and its tracked future; session directory
/// creation is the only fallible I/O, a typed error before launch.
pub(crate) fn prepare_member(
    wiring: &SubagentWiring,
    request: &SpawnRequest,
    id: &str,
    prompt: String,
) -> Result<TrackedChild, String> {
    let hub = &wiring.hub;
    let session_path = hub.child_session_path(id);
    if let Some(path) = &session_path {
        let dir = path
            .parent()
            .ok_or_else(|| format!("child session path `{}` has no directory", path.display()))?;
        std::fs::create_dir_all(dir).map_err(|source| {
            format!(
                "cannot create the child session dir {}: {source}",
                dir.display()
            )
        })?;
    }
    let settings = hub.settings();
    let child_wiring = SubagentWiring {
        hub: hub.clone(),
        depth: wiring.depth.saturating_add(1),
    };
    let knobs = ChildKnobs {
        id: id.to_string(),
        workspace_root: settings.workspace_root.clone(),
        max_turns: request.max_turns,
        max_total_tokens: request.slice,
        per_request_tokens: settings.per_request_tokens,
        limits: settings.limits,
        session_path,
        idle_timeout: settings.idle_timeout,
        step_attempts: settings.step_attempts,
        step_patience: settings.step_patience,
        spawn: Some(child_wiring),
        deny_read: Vec::new(),
        deny_write: Vec::new(),
    };
    let child: ChildRun = prepare_child(
        &request.profile,
        &settings.default_system_prompt,
        Some(SUBAGENT_OVERLAY),
        knobs,
    )?;
    let future = hub.spawn_future(child, prompt)?;
    Ok(TrackedChild::new(
        hub.clone(),
        id.to_string(),
        future,
        request.timeout_secs,
    ))
}

/// Admits the request fail-closed, prepares every member, and hands
/// back the driving stream. Nothing launches on refusal — zero
/// provider calls, zero side effects.
pub(crate) fn drive(request: SpawnRequest, wiring: &SubagentWiring) -> ToolStream {
    let hub = wiring.hub.clone();
    let count = request.members.len() as u32;
    let profile_name = request.profile.name.clone();
    let batch = AdmitBatch {
        slice: request.slice,
        count,
    };
    // The blocking path refuses on a saturated pool: this call awaits
    // the whole wave, so a refused batch is the caller's decision to
    // retry — a queue would hang it.
    let ids = match hub.admit(wiring.depth, batch, &profile_name, PoolFull::Refuse) {
        Ok(ids) => ids,
        Err(refusal) => {
            return craft_tools::event::error_stream(ToolError::SubagentFailed {
                message: refusal,
            });
        }
    };
    let mut queued: VecDeque<TrackedChild> = VecDeque::new();
    for (id, prompt) in ids.iter().zip(request.members.iter()) {
        match prepare_member(wiring, &request, id, prompt.clone()) {
            Ok(child) => queued.push_back(child),
            Err(detail) => {
                hub.abandon(&ids);
                return craft_tools::event::error_stream(ToolError::SubagentFailed {
                    message: format!("spawn failed before launch: {detail}"),
                });
            }
        }
    }
    SpawnStream {
        hub,
        profile: profile_name,
        max_turns: request.max_turns,
        timeout_secs: request.timeout_secs,
        batch: request.batch,
        launched: FuturesUnordered::new(),
        queued,
        reports: Vec::new(),
        seen_turns: HashMap::new(),
        progress: VecDeque::new(),
        done: false,
    }
    .boxed()
}

/// The one driver: launch up to the concurrency cap, queue the rest
/// FIFO, settle each completion, and emit exactly one terminal —
/// the single envelope or the batch aggregate.
pub(crate) struct SpawnStream {
    hub: Arc<SubagentHub>,
    profile: String,
    max_turns: u32,
    timeout_secs: Option<u64>,
    batch: bool,
    launched: FuturesUnordered<TrackedChild>,
    queued: VecDeque<TrackedChild>,
    reports: Vec<(String, ChildReport, u64)>,
    seen_turns: HashMap<String, u32>,
    progress: VecDeque<String>,
    done: bool,
}

impl SpawnStream {
    fn running_room(&self) -> bool {
        self.hub.working() < self.hub.caps().max_concurrency
    }

    fn launch_queued(&mut self) {
        while self.running_room() {
            let Some(mut child) = self.queued.pop_front() else {
                break;
            };
            let id = child.id().to_string();
            child.mark_launched();
            self.seen_turns.insert(id.clone(), 0);
            self.hub.launch(&id, self.max_turns, self.timeout_secs);
            self.launched.push(child);
            self.progress.push_back(format!("{id} started"));
        }
    }

    /// Live per-child turn milestones, one per poll.
    fn next_turn_milestone(&mut self) -> Option<String> {
        let ids: Vec<String> = self
            .launched
            .iter()
            .map(|child| child.id().to_string())
            .collect();
        for id in ids {
            let turn = self.hub.child_turn(&id);
            let seen = self.seen_turns.get(&id).copied().unwrap_or(0);
            if turn > seen {
                self.seen_turns.insert(id.clone(), turn);
                return Some(format!("{id} turn {turn}"));
            }
        }
        None
    }

    /// The single terminal; the contract enforcer guarantees it is
    /// the last item.
    fn terminal(&mut self) -> Option<Result<ToolEvent, ToolError>> {
        self.done = true;
        if self.batch {
            let handoffs = self.handoffs();
            return Some(Ok(ToolEvent::Terminal(ToolResult {
                output: render_batch(&handoffs),
            })));
        }
        let handoff = self.handoffs().into_iter().next()?;
        if handoff.stop.is_error() {
            return Some(Err(ToolError::SubagentFailed {
                message: handoff.render(),
            }));
        }
        Some(Ok(ToolEvent::Terminal(ToolResult {
            output: handoff.render(),
        })))
    }
    /// The settled members as envelopes, in settle order.
    fn handoffs(&self) -> Vec<Handoff> {
        self.reports
            .iter()
            .map(|(id, report, remaining)| {
                report.clone().into_handoff(id, &self.profile, *remaining)
            })
            .collect()
    }
}

impl futures::Stream for SpawnStream {
    type Item = Result<ToolEvent, ToolError>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if this.done {
                return Poll::Ready(None);
            }
            if let Some(text) = this.progress.pop_front() {
                return Poll::Ready(Some(Ok(ToolEvent::Progress(ToolProgress::stdout(text)))));
            }
            this.launch_queued();
            if this.launched.is_empty() {
                return Poll::Ready(this.terminal());
            }
            match Pin::new(&mut this.launched).poll_next(cx) {
                Poll::Ready(Some((id, report))) => {
                    let remaining = this.hub.settle(&id, &report);
                    this.reports.push((id.clone(), report, remaining));
                    this.progress.push_back(format!("{id} finished"));
                }
                Poll::Ready(None) => return Poll::Ready(this.terminal()),
                Poll::Pending => match this.next_turn_milestone() {
                    Some(milestone) => {
                        this.progress.push_back(milestone);
                    }
                    None => return Poll::Pending,
                },
            }
        }
    }
}
