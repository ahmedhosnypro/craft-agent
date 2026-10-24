//! Hub admission and settle: the fail-closed gate every spawn passes
//! before any spend (depth, total, concurrency, and the joint budget
//! reserve), the exactly-once settle that folds each child's spend
//! into the ledger and queues its envelope event, and the timeout
//! report built from the noted spend.

use craft_core::types::Usage;

use crate::envelope::ChildReport;
use crate::events::RuntimeEvent;
use crate::subagent::{SubagentHub, SubagentWiring, tokens};
use craft_core::subagent::{ChildStop, DETAIL_MAX_CHARS};

/// One admission's batch shape: the per-child token budget and the
/// member count, reserved jointly before any launch. A None budget is
/// unlimited and reserves nothing.
#[derive(Debug, Clone, Copy)]
pub struct AdmitBatch {
    pub slice: Option<u64>,
    pub count: u32,
}

/// What a spawn does when the pool is saturated: the blocking path
/// refuses (its caller awaits the whole wave, so a queue would hang
/// it), the detached path queues (its caller already returned at
/// admission and collects by poll).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolFull {
    Refuse,
    Queue,
}

/// Envelope event queued for the runner; the parent turn is stamped
/// at the drain.
pub(crate) enum HubEvent {
    Started {
        id: String,
        profile: String,
        max_turns: u32,
        max_tokens: Option<u64>,
        timeout_secs: Option<u64>,
    },
    Finished {
        id: String,
        profile: String,
        stop_reason: String,
        detail: String,
        usage: Usage,
        turns: u32,
        session_path: Option<String>,
    },
}

impl HubEvent {
    pub(crate) fn stamped(self, turn: u32) -> RuntimeEvent {
        match self {
            HubEvent::Started {
                id,
                profile,
                max_turns,
                max_tokens,
                timeout_secs,
            } => RuntimeEvent::SubagentStarted {
                turn,
                id,
                profile,
                max_turns,
                max_tokens,
                timeout_secs,
            },
            HubEvent::Finished {
                id,
                profile,
                stop_reason,
                detail,
                usage,
                turns,
                session_path,
            } => RuntimeEvent::SubagentFinished {
                turn,
                id,
                profile,
                stop_reason,
                detail,
                usage,
                turns,
                session_path,
            },
        }
    }
}

impl SubagentHub {
    /// Fail-closed admission, before any spend. Every refusing
    /// dimension is named; zero provider calls are made on refusal.
    /// A batch reserves every member's slice up front — nothing
    /// launches unless the whole batch fits. A saturated pool refuses
    /// a blocking spawn and queues a detached one; depth, total, and
    /// budget refuse both.
    pub fn admit(
        &self,
        depth: u8,
        batch: AdmitBatch,
        profile: &str,
        on_full: PoolFull,
    ) -> Result<Vec<String>, String> {
        let AdmitBatch { slice, count } = batch;
        let caps = self.settings().caps;
        let mut state = self.lock_state();
        if slice == Some(0) {
            return Err(String::from(
                "token budget: no remaining budget for subagents; give the spawn a smaller maxTokens or finish with less spend",
            ));
        }
        if u16::from(depth) + 1 > u16::from(caps.max_depth) {
            return Err(format!(
                "depth: subagents may nest {} deep; this spawn came from depth {depth}",
                caps.max_depth
            ));
        }
        if state.spawned_total.saturating_add(count) > caps.max_total {
            return Err(format!(
                "total: the run admits at most {} subagents; {} were already spawned",
                caps.max_total, state.spawned_total
            ));
        }
        if state.running >= caps.max_concurrency && on_full == PoolFull::Refuse {
            return Err(format!(
                "concurrency: {} subagents are already running (cap {})",
                state.running, caps.max_concurrency
            ));
        }
        let need = slice.unwrap_or(0).saturating_mul(u64::from(count));
        if let Some(cap) = self.settings().run_token_cap {
            let spent = tokens(&state.parent_spent).saturating_add(SubagentHub::committed(&state));
            if spent.saturating_add(need) > cap {
                return Err(format!(
                    "token budget: this spawn needs {need} tokens but only {} remain of the run budget",
                    cap.saturating_sub(spent)
                ));
            }
        }
        let mut ids = Vec::with_capacity(count as usize);
        for _ in 0..count {
            state.next_id = state.next_id.saturating_add(1);
            let id = format!("agent-{}", state.next_id);
            state.admitted.insert(
                id.clone(),
                crate::subagent::AdmittedChild {
                    slice,
                    profile: profile.to_string(),
                },
            );
            ids.push(id);
        }
        state.spawned_total = state.spawned_total.saturating_add(count);
        state.in_flight = state.in_flight.saturating_add(need);
        Ok(ids)
    }
}

impl SubagentHub {
    /// The child transcript path: `<sessions>/<parent-stem>/agents/
    /// agent-N/session.jsonl`; an in-memory parent keeps children in
    /// memory too.
    pub fn child_session_path(&self, id: &str) -> Option<std::path::PathBuf> {
        let parent = self.settings().parent_session.as_ref()?;
        let stem = parent.file_stem()?.to_string_lossy().into_owned();
        Some(
            parent
                .parent()?
                .join(stem)
                .join("agents")
                .join(id)
                .join("session.jsonl"),
        )
    }

    pub fn running(&self) -> u32 {
        self.lock_state().running
    }

    /// Per-turn spend of a live child, for the timeout report.
    pub fn note_child_usage(&self, id: &str, usage: Usage) {
        let mut state = self.lock_state();
        let entry = state
            .child_usage
            .entry(id.to_string())
            .or_insert_with(crate::budget::zero_usage);
        *entry = crate::budget::sum_usage(*entry, usage);
    }

    /// Turn milestones for the spawn tool's progress lines.
    pub fn note_child_turn(&self, id: &str, turn: u32) {
        self.lock_state().child_turn.insert(id.to_string(), turn);
    }

    pub fn child_turn(&self, id: &str) -> u32 {
        self.lock_state().child_turn.get(id).copied().unwrap_or(0)
    }

    /// Releases admitted children that never launched: the reserve
    /// returns, the ids stay consumed (id-churn bound), no events
    /// fire.
    pub fn abandon(&self, ids: &[String]) {
        let mut state = self.lock_state();
        for id in ids {
            if let Some(child) = state.admitted.remove(id) {
                state.in_flight = state.in_flight.saturating_sub(child.slice.unwrap_or(0));
            }
        }
    }

    /// Releases one queued (never launched) child.
    pub fn release(&self, id: &str) {
        self.abandon(std::slice::from_ref(&id.to_string()));
    }

    /// A member took a free slot: counts it running and queues the
    /// `subagent_started` envelope event.
    pub fn launch(&self, id: &str, max_turns: u32, timeout_secs: Option<u64>) {
        let mut state = self.lock_state();
        state.running = state.running.saturating_add(1);
        let (slice, profile) = match state.admitted.get(id) {
            Some(child) => (child.slice, child.profile.clone()),
            None => return,
        };
        state.events.push_back(HubEvent::Started {
            id: id.to_string(),
            profile,
            max_turns,
            max_tokens: slice,
            timeout_secs,
        });
    }

    /// The honest remaining run budget over an already-held state;
    /// u64::MAX is the unlimited sentinel.
    pub(crate) fn remaining(cap: &Option<u64>, state: &crate::subagent::HubState) -> u64 {
        match cap {
            Some(cap) => cap
                .saturating_sub(tokens(&state.parent_spent))
                .saturating_sub(SubagentHub::committed(state)),
            None => u64::MAX,
        }
    }

    /// Exactly-once settle per child id — a second settle for the
    /// same id is ignored. Folds the child's spend into the ledger,
    /// frees its slot and reserve, queues the `subagent_finished`
    /// event, and returns the honest post-child remaining budget.
    pub fn settle(&self, id: &str, report: &ChildReport) -> u64 {
        let mut state = self.lock_state();
        let Some(child) = state.admitted.remove(id) else {
            return Self::remaining(&self.settings().run_token_cap, &state);
        };
        if !state.settled.insert(id.to_string()) {
            state.admitted.insert(id.to_string(), child);
            return Self::remaining(&self.settings().run_token_cap, &state);
        }
        state.child_settled = crate::budget::sum_usage(state.child_settled, report.usage);
        state.in_flight = state.in_flight.saturating_sub(child.slice.unwrap_or(0));
        state.running = state.running.saturating_sub(1);
        let detail = crate::envelope::cap_chars(&report.detail, DETAIL_MAX_CHARS);
        state.events.push_back(HubEvent::Finished {
            id: id.to_string(),
            profile: child.profile,
            stop_reason: report.stop.as_str().to_string(),
            detail,
            usage: report.usage,
            turns: report.turns,
            session_path: report.session_path.clone(),
        });
        Self::remaining(&self.settings().run_token_cap, &state)
    }

    /// A report built from the child's noted spend: the timeout and
    /// drop shapes share it.
    fn noted_report(&self, id: &str, stop: ChildStop, detail: String) -> ChildReport {
        let mut usage = self
            .lock_state()
            .child_usage
            .get(id)
            .copied()
            .unwrap_or_else(crate::budget::zero_usage);
        usage.incomplete = true;
        ChildReport {
            stop,
            usage,
            turns: self.child_turn(id),
            final_text: String::new(),
            detail,
            session_path: self
                .child_session_path(id)
                .map(|path| path.display().to_string()),
        }
    }

    /// The timeout report: the noted spend marked unverifiable; the
    /// child future is already dropped, so its in-flight processes
    /// died with it.
    pub fn timed_out_report(&self, id: &str, secs: u64) -> ChildReport {
        self.noted_report(
            id,
            ChildStop::TimedOut,
            format!("subagent exceeded its wall-clock cap ({secs}s)"),
        )
    }

    /// The settle-on-drop backstop: a launched child whose future was
    /// dropped mid-drive (the parent run itself died) settles with a
    /// typed spawn error so its reserve can never leak.
    pub(crate) fn dropped_report(&self, id: &str) -> ChildReport {
        self.noted_report(
            id,
            ChildStop::SpawnError,
            String::from("subagent dropped before completion"),
        )
    }
}

/// Structural spawn admission for a registry being built: a runner at
/// `depth` may carry the spawn pair only while one more level fits.
pub fn spawn_admits(wiring: &SubagentWiring) -> bool {
    u16::from(wiring.depth) < u16::from(wiring.hub.caps().max_depth)
}
