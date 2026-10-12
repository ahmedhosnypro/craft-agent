//! Provider adapters: the model backends behind one streaming trait.

pub mod anthropic;
pub mod backend;
pub mod gemini;
mod lane_driver;
pub mod openai;
mod retry;
pub mod transport;
