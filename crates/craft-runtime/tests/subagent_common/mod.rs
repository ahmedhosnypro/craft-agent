//! Shared doubles for the subagent corpus.
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod provider;
pub mod rig;

pub use provider::{FakeProvider, Reply, delayed, text_reply, tool_reply, usage};
pub use rig::{
    Rig, RigSpec, catalog, finished, parent_run, pipeline, rig, run, scout, slowpoke, spawner,
    started, tool_results, workspace, writer,
};
