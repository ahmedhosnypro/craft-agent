//! Deterministic collection helpers for the digest mission.

pub mod cli;
pub mod dups;
pub mod dups_rank;
pub mod dups_scan;
pub mod error;
pub mod fetch;
pub mod http;
pub mod prs;
pub mod prs_run;
pub mod time;

use serde::Serialize;

/// A cap that clipped a report, emitted so callers never hunt for limits.
#[derive(Debug, Serialize)]
pub struct Truncation {
    pub field: String,
    pub cap: usize,
    pub hint: String,
}

impl Truncation {
    pub fn new(field: &str, cap: usize, hint: &str) -> Self {
        Truncation {
            field: field.to_string(),
            cap,
            hint: crate::error::bounded(hint),
        }
    }
}
