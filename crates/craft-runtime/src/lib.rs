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
//!
//! Subagents run through the [`subagent`] hub: the parent registry
//! carries the `spawn_agent` / `spawn_agents` pair behind a hub
//! wiring, children are full runners over the same backend with
//! their own persona, slice, and transcript, and their spend folds
//! into the parent budget through the shared ledger.

pub mod child;
pub mod events;
pub mod outcome;
pub mod profile;
pub mod runner;
pub mod session;
pub mod subagent;
mod subagent_caps;

mod detached;
mod envelope;
mod polls_tool;
mod subagent_admit;

mod agent_tool;
mod agents_tool;
pub(crate) mod spawn;
pub(crate) mod spawn_args;

mod budget;
mod call;
pub mod compact;
mod finish;
mod guard;
pub mod limits;
mod prepare;
mod request;
mod step;
mod trim;
mod truncate;

pub mod retry;

pub use child::{ChildKnobs, ChildRun, prepare_child};
pub use craft_core::subagent::SUBAGENT_OVERLAY;
pub use detached::{DETACHED_DRAIN, drain_detached};
pub use envelope::ChildReport;
pub use events::{CancelCategory, RuntimeEvent, ToolStreamName, redact_event};
pub use limits::{
    ContextKnobs, ENV_MAX_CONTEXT_TOKENS, ENV_MAX_INPUT_TOKENS, ENV_MAX_OUTPUT_TOKENS,
    ENV_THINKING_LEVEL,
};
pub use outcome::{ExitStatus, RunOutcome};
pub use runner::{DEFAULT_REQUEST_TOKENS, RunConfig, Runner};
pub use subagent::{SubagentCaps, SubagentHub, SubagentSettings, SubagentWiring};
pub use subagent_admit::{AdmitBatch, PoolFull};
