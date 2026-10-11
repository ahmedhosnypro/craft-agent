//! Digest mission: collectors, prompt assembly, and publish preparation.

pub mod bundle;
pub mod clone_runner;
pub mod collect;
pub mod dry_run;
pub mod error;
pub mod jscpd;
pub mod jscpd_runner;
pub mod pipeline;
pub mod prompt;
pub mod render;
pub mod sanitize;
pub mod scrub;
pub mod turn;
pub mod window;

pub use error::DigestError;
