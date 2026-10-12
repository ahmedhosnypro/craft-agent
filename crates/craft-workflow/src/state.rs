//! The engine state machine: one slot per step, unmet dependency counts,
//! and the fail-closed pool.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::plan::WorkflowPlan;
use crate::pool::Pool;
use crate::status::StepConclusion;
use crate::status::{StepStatus, StepUsage};

pub(crate) enum Slot {
    Pending,
    Running,
    Done(StepStatus),
}

pub(crate) struct EngineState {
    pub slots: BTreeMap<String, Slot>,
    pub unmet: BTreeMap<String, u32>,
    pub ready: VecDeque<String>,
    pub input_hashes: BTreeMap<String, String>,
    pub attempts: BTreeMap<String, u32>,
    pub attempt_usage: BTreeMap<String, StepUsage>,
    pub counted: BTreeSet<String>,
    pub first_failure: Option<(String, StepConclusion)>,
    pub stopping: bool,
    pub budget_refused: bool,
    pub pool: Pool,
    pub total_usage: StepUsage,
}

impl EngineState {
    pub(crate) fn new(plan: &WorkflowPlan) -> Self {
        let mut slots = BTreeMap::new();
        let mut unmet = BTreeMap::new();
        let mut ready = VecDeque::new();
        for spec in &plan.steps {
            let needs = spec.needs.len() as u32;
            slots.insert(spec.id.clone(), Slot::Pending);
            unmet.insert(spec.id.clone(), needs);
            if needs == 0 {
                ready.push_back(spec.id.clone());
            }
        }
        Self {
            slots,
            unmet,
            ready,
            input_hashes: BTreeMap::new(),
            attempts: BTreeMap::new(),
            attempt_usage: BTreeMap::new(),
            counted: BTreeSet::new(),
            first_failure: None,
            stopping: false,
            budget_refused: false,
            pool: Pool::new(plan.limits),
            total_usage: StepUsage::default(),
        }
    }

    pub(crate) fn settled_preview(&self, id: &str) -> Option<String> {
        match self.slots.get(id) {
            Some(Slot::Done(StepStatus::Settled { final_message, .. })) => {
                Some(final_message.clone())
            }
            _ => None,
        }
    }

    pub(crate) fn is_settled_success(&self, id: &str) -> bool {
        matches!(
            self.slots.get(id),
            Some(Slot::Done(StepStatus::Settled { conclusion, .. })) if conclusion.is_success()
        )
    }
}
