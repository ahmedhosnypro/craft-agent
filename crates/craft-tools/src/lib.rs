//! Tool trait and the builtin tools the agent can call: bash, read_file,
//! write_file, edit_file, glob, grep, and gh. This crate is
//! self-contained — the runtime bridges its events into the model-event
//! world.

pub mod bash;
pub mod edit;
pub mod envscrub;
pub mod error;
pub mod event;
pub mod gh;
pub mod glob;
pub mod grep;
pub mod pathguard;
pub mod proc;
pub mod read;
pub mod registry;
pub mod schema;
pub mod tool;
pub mod util;
pub mod walk;
pub mod write;

pub use error::ToolError;
pub use event::{OutStream, ToolEvent, ToolProgress, ToolResult, ToolStream};
pub use registry::{Registry, ToolSpec};
pub use schema::Schema;
pub use tool::{EnvSource, Tool, ToolCtx, ToolLimits};

/// Appended to any bounded output that was cut at the cap.
pub const TRUNCATION_MARKER: &str = "[output truncated]";
