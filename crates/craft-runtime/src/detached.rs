//! Detached subagents: a spawn whose tool call returns at admission, a
//! bounded pool of children running against the concurrency cap, and
//! reports parked in the hub until the parent polls for them. A
//! saturated pool queues a detached spawn instead of refusing it —
//! the caller already returned at admission — and continuations a
//! running child dispatched drain before fresh dispatches, so a PR's
//! next stage never waits behind pipelines that have not started. The
//! fail-closed laws of the blocking spawn hold unchanged: admission
//! refuses before any spend, every child carries its own wall clock,
//! and every admitted id ends as `completed`, `timed_out`, or
//! `spawn_error` — never lost.

use std::collections::VecDeque;
use std::time::Duration;

use craft_tools::event::ToolStream;
use craft_tools::{ToolError, ToolEvent, ToolResult};
use futures::StreamExt;

use crate::spawn::{TrackedChild, prepare_member};
use crate::spawn_args::SpawnRequest;
use crate::subagent::{BgMember, ParkedChild, SubagentHub, SubagentWiring};
use crate::subagent_admit::{AdmitBatch, PoolFull};

/// The bounded drain a run's end holds for detached children whose
/// wall clock is still live: long enough to collect a nearly-finished
/// pool, short enough to never hold the terminal event hostage.
pub const DETACHED_DRAIN: std::time::Duration = std::time::Duration::from_secs(60);

impl SubagentHub {
    /// Parks detached members and opens pool slots while the
    /// concurrency cap allows. Admission already happened fail-closed,
    /// so this only dispatches: a saturated pool queues the caller's
    /// members instead of refusing the call. A member a running child
    /// dispatched is a continuation, and it waits in the queue that
    /// drains first.
    pub(crate) fn accept_detached(&self, queued: VecDeque<BgMember>, continuation: bool) {
        let mut state = self.lock_state();
        if continuation {
            state.bg_continuation.extend(queued);
        } else {
            state.bg_fresh.extend(queued);
        }
        drop(state);
        self.launch_queued();
    }

    /// Launches queued members while slots are free, continuations
    /// before fresh dispatches. A launch owns no parent turn: its task
    /// settles the child and re-enters here, so the pool drains
    /// without the parent awaiting any wave.
    pub(crate) fn launch_queued(&self) {
        loop {
            if self.running() >= self.caps().max_concurrency {
                return;
            }
            let member = {
                let mut state = self.lock_state();
                state
                    .bg_continuation
                    .pop_front()
                    .or_else(|| state.bg_fresh.pop_front())
            };
            let Some(member) = member else {
                return;
            };
            let hub = member.child.hub();
            let mut child = member.child;
            let id = child.id().to_string();
            let timeout_secs = child.timeout_secs();
            // Marked launched before the task owns it, so an aborted
            // task settles through the drop backstop instead of
            // silently releasing its reserve.
            child.mark_launched();
            self.launch(&id, member.max_turns, timeout_secs);
            tokio::spawn(drive_member(hub, child));
        }
    }

    /// Settles a detached child and parks its report. The hub's settle
    /// is exactly-once, so a report that loses the race is recorded
    /// without folding spend twice.
    pub(crate) fn park_report(&self, id: &str, report: crate::envelope::ChildReport) {
        let profile = self
            .lock_state()
            .admitted
            .get(id)
            .map(|child| child.profile.clone())
            .unwrap_or_default();
        let remaining = self.settle(id, &report);
        self.lock_state().bg_parked.insert(
            id.to_string(),
            ParkedChild {
                report,
                profile,
                budget_remaining: remaining,
            },
        );
    }

    /// Takes parked reports by id, or every parked report when the list
    /// is empty. The envelopes come back in id order; a requested id
    /// that is still outstanding simply does not arrive.
    pub(crate) fn take_parked(&self, ids: &[String]) -> Vec<(String, ParkedChild)> {
        let mut state = self.lock_state();
        let picked: Vec<String> = if ids.is_empty() {
            let mut all: Vec<String> = state.bg_parked.keys().cloned().collect();
            all.sort();
            all
        } else {
            ids.to_vec()
        };
        picked
            .into_iter()
            .filter_map(|id| state.bg_parked.remove(&id).map(|report| (id, report)))
            .collect()
    }

    /// Every detached child neither settled nor released, in id order:
    /// the pool's outstanding work.
    pub(crate) fn outstanding(&self) -> Vec<String> {
        let state = self.lock_state();
        let mut ids: Vec<String> = state
            .admitted
            .keys()
            .filter(|id| !state.settled.contains(*id))
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    /// True while the hub holds this id as admitted queued work. A
    /// resumed run admits nothing, so older ids read as unknown.
    pub(crate) fn knows(&self, id: &str) -> bool {
        let state = self.lock_state();
        state.admitted.contains_key(id) || state.bg_parked.contains_key(id)
    }
}

/// The launched-child task: run the child to its report, park it for the
/// parent's next poll, then open the next queued slot. A dropped task
/// settles its child through the `TrackedChild` drop backstop.
async fn drive_member(hub: std::sync::Arc<SubagentHub>, child: TrackedChild) {
    let id = child.id().to_string();
    let (_id, report) = child.await;
    hub.park_report(&id, report);
    hub.launch_queued();
}

/// Drains the pool at run end: waits bounded for outstanding children,
/// then settles any straggler as dropped so every admitted id ends
/// typed and its ledger row is honest.
pub async fn drain_detached(wiring: &SubagentWiring, wait: Duration) {
    let hub = &wiring.hub;
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        hub.launch_queued();
        if hub.outstanding().is_empty() {
            return;
        }
        let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) else {
            break;
        };
        if remaining.is_zero() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100).min(remaining)).await;
    }
    for id in hub.outstanding() {
        hub.park_report(&id, hub.dropped_report(&id));
    }
}

/// Admits a detached request fail-closed and hands back the driving
/// stream. Nothing launches on refusal — zero provider calls, zero
/// side effects.
pub(crate) fn drive(request: SpawnRequest, wiring: &SubagentWiring) -> ToolStream {
    let hub = wiring.hub.clone();
    let count = request.members.len() as u32;
    let profile_name = request.profile.name.clone();
    let batch = AdmitBatch {
        slice: request.slice,
        count,
    };
    let ids = match hub.admit(wiring.depth, batch, &profile_name, PoolFull::Queue) {
        Ok(ids) => ids,
        Err(refusal) => {
            return craft_tools::event::error_stream(ToolError::SubagentFailed {
                message: refusal,
            });
        }
    };
    if tokio::runtime::Handle::try_current().is_err() {
        hub.abandon(&ids);
        return craft_tools::event::error_stream(ToolError::SubagentFailed {
            message: String::from("detached spawn needs an async runtime; this run has none"),
        });
    }
    let mut queued: VecDeque<BgMember> = VecDeque::new();
    for (id, prompt) in ids.iter().zip(request.members.iter()) {
        match prepare_member(wiring, &request, id, prompt.clone()) {
            Ok(child) => queued.push_back(BgMember {
                child,
                max_turns: request.max_turns,
            }),
            Err(detail) => {
                hub.abandon(&ids);
                return craft_tools::event::error_stream(ToolError::SubagentFailed {
                    message: format!("spawn failed before launch: {detail}"),
                });
            }
        }
    }
    // A dispatch from a running child continues that child's own
    // work, so it queues ahead of dispatches the root parent has not
    // started yet.
    let continuation = wiring.depth > 0;
    hub.accept_detached(queued, continuation);
    futures::stream::once(async move {
        Ok(ToolEvent::Terminal(ToolResult {
            output: accepted_line(&hub, &ids),
        }))
    })
    .boxed()
}

/// The immediate terminal: what was accepted, and how to read it back.
fn accepted_line(hub: &SubagentHub, ids: &[String]) -> String {
    let (running, continuation, fresh) = {
        let state = hub.lock_state();
        (
            state.running,
            state.bg_continuation.len(),
            state.bg_fresh.len(),
        )
    };
    let mut out = format!(
        "detached: {} accepted, {} running, {} queued ({} continuation, {} fresh); collect envelopes with poll_agents\n",
        ids.len(),
        running,
        continuation + fresh,
        continuation,
        fresh,
    );
    for id in ids {
        out.push_str("accepted: ");
        out.push_str(id);
        out.push('\n');
    }
    out
}
