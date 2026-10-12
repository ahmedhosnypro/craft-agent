//! The fail-closed pool: step count, total turns, and total tokens checked
//! together before any admission.

use crate::plan::{Limits, StepSpec};
use crate::status::StepUsage;

/// Which pool dimension refused an admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolRefusal {
    Steps,
    Turns,
    Tokens,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pool {
    pub limits: Limits,
    started: u32,
    used_turns: u64,
    used_tokens: u64,
}

impl Pool {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            started: 0,
            used_turns: 0,
            used_tokens: 0,
        }
    }

    /// Re-seed from prior consumption (resume: prior usage still counts).
    pub fn seed(&mut self, started: u32, turns: u64, tokens: u64) {
        self.started = started;
        self.used_turns = turns;
        self.used_tokens = tokens;
    }

    /// Admission pre-check: a step may start only if every dimension holds.
    /// Turn reservation charges the step's own max-turns ceiling up front so
    /// a resumed run never re-admits work the budget already refused.
    pub fn check(&self, step: &StepSpec) -> Result<(), PoolRefusal> {
        if self.started >= self.limits.max_steps {
            Err(PoolRefusal::Steps)
        } else if self.used_turns.saturating_add(u64::from(step.max_turns))
            > self.limits.max_total_turns
        {
            Err(PoolRefusal::Turns)
        } else if self.used_tokens >= self.limits.max_total_tokens {
            Err(PoolRefusal::Tokens)
        } else {
            Ok(())
        }
    }

    pub fn start(&mut self) {
        self.started = self.started.saturating_add(1);
    }

    pub fn settle(&mut self, usage: StepUsage) {
        self.used_turns = self.used_turns.saturating_add(usage.turns);
        self.used_tokens = self.used_tokens.saturating_add(usage.total_tokens());
    }

    pub fn totals(&self) -> (u32, u64, u64) {
        (self.started, self.used_turns, self.used_tokens)
    }
}
