//! Exit codes and the terminal run record.

use std::path::PathBuf;

use craft_core::types::Usage;

use crate::events::CancelCategory;

/// Process exit codes: 0 ok · 1 error · 2 max-turns · 3 budget ·
/// 4 permission/sensitive-path denial. `Stationarity` shares code 2
/// with `MaxTurns` — both are motion-budget aborts; the cancellation
/// category distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Ok,
    Error,
    MaxTurns,
    Stationarity,
    Budget,
    Denied,
}

impl ExitStatus {
    pub fn code(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Error => 1,
            Self::MaxTurns | Self::Stationarity => 2,
            Self::Budget => 3,
            Self::Denied => 4,
        }
    }
}

/// The terminal record of one run: what the app layer prints and exits with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub exit: ExitStatus,
    pub category: CancelCategory,
    pub detail: String,
    pub usage: Usage,
    pub turns: u32,
    pub session_path: Option<PathBuf>,
}

impl RunOutcome {
    pub fn exit_code(&self) -> i32 {
        self.exit.code()
    }
}
