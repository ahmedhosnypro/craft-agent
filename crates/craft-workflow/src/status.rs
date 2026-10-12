//! Step vocabulary, the step table, and the terminal run report.

use serde::{Deserialize, Serialize};

/// One step's terminal conclusion, 1:1 with the run exit taxonomy:
/// 0 ok · 1 error · 2 max-turns/stationarity · 3 budget · 4 denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepConclusion {
    Success,
    Error,
    MaxTurns,
    Stationarity,
    Budget,
    Denied,
}

impl StepConclusion {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Error => 1,
            Self::MaxTurns | Self::Stationarity => 2,
            Self::Budget => 3,
            Self::Denied => 4,
        }
    }

    pub fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }
}

/// Turn/token accounting; every arithmetic on it is saturating.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepUsage {
    pub turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

impl StepUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.reasoning_tokens)
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self {
            turns: self.turns.saturating_add(other.turns),
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            reasoning_tokens: self.reasoning_tokens.saturating_add(other.reasoning_tokens),
        }
    }
}

/// Why a step never ran; skips are visible, never silent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkipReason {
    DependencyFailed { dep: String },
    Unreachable,
}

/// One step's row in the final table: settled outcomes or a visible skip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepStatus {
    Settled {
        conclusion: StepConclusion,
        usage: StepUsage,
        final_message: String,
    },
    Skipped {
        reason: SkipReason,
    },
}

impl StepStatus {
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Settled { conclusion, .. } => Some(conclusion.exit_code()),
            Self::Skipped { .. } => None,
        }
    }
}

/// Terminal run conclusion; the exit code is the first failing step's code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunConclusion {
    Completed,
    Failed {
        step: String,
        conclusion: StepConclusion,
    },
    BudgetExhausted,
}

impl RunConclusion {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Completed => 0,
            Self::Failed { conclusion, .. } => conclusion.exit_code(),
            Self::BudgetExhausted => 3,
        }
    }
}

/// One step's row in the report the caller serializes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepReport {
    pub id: String,
    pub status: StepStatus,
}

impl StepReport {
    pub fn exit_code(&self) -> Option<i32> {
        self.status.exit_code()
    }
}

/// The `workflow_finished` record: step table, conclusion, and totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowReport {
    pub run_id: String,
    pub name: String,
    pub conclusion: RunConclusion,
    pub exit_code: i32,
    pub failed_step: Option<String>,
    pub steps: Vec<StepReport>,
    pub total_usage: StepUsage,
}

impl WorkflowReport {
    pub fn is_success(&self) -> bool {
        matches!(self.conclusion, RunConclusion::Completed)
    }
}
