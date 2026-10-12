//! The agent run loop crate: drives a mounted provider and the tool
//! registry turn by turn under fail-closed budgets, keeps the
//! append-only session record verbatim (tool results older than the
//! result window ride later prompts as size stubs, oversized ones as
//! head plus a spill pointer), and emits the NDJSON run-event contract
//! the headless writer serializes.
//!
//! The app layer owns env and CLI wiring; it constructs the provider,
//! the registry (its tool set is the permission posture), and a
//! [`RunConfig`], then drives [`Runner::run`], serializing every
//! [`RuntimeEvent`] with `serde_json::to_string` — one line each — and
//! exiting with [`RunOutcome::exit_code`].

pub mod events;
pub mod outcome;
pub mod runner;
pub mod session;

mod budget;
mod call;
mod guard;
mod prepare;
mod request;
mod step;
mod trim;
mod truncate;

pub mod retry;

pub use events::{CancelCategory, RuntimeEvent, ToolStreamName};
pub use outcome::{ExitStatus, RunOutcome};
pub use runner::{RunConfig, Runner};
