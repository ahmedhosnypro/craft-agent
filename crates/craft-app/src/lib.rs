//! craft-agent library surface: the CLI logic the binary and the test
//! corpora share. `main.rs` stays a thin parser-and-dispatch shell.

pub mod cli;
pub mod config;
pub mod digest_cmd;
pub mod error;
pub mod run_cmd;
pub mod stream_json;

pub use craft_core::types::Usage;
pub use craft_runtime::{CancelCategory, ExitStatus, RunOutcome, RuntimeEvent, ToolStreamName};

pub use error::AppError;
