//! Shared doubles for the workflow corpus: a scripted provider over
//! the real `Provider` seam and a minimal step-agent profile fixture.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use craft_core::agents::{AgentFile, PromptMode};
use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, Provider, boxed};
use craft_core::types::{ModelRequest, StopReason, Usage};
use futures::stream;

/// One scripted provider call.
#[derive(Clone)]
pub enum Reply {
    /// Stream these events in order, then end the stream.
    Events(Vec<Result<ModelEvent, ProviderError>>),
    /// `stream()` itself fails before producing any event.
    Fail(ProviderError),
}

/// Scripted provider: plays replies in order, then repeats the last
/// reply forever, recording every request it received.
pub struct FakeProvider {
    replies: Mutex<VecDeque<Reply>>,
    seen: Mutex<Vec<ModelRequest>>,
}

impl FakeProvider {
    pub fn new(replies: Vec<Reply>) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into()),
            seen: Mutex::new(Vec::new()),
        })
    }

    pub fn calls(&self) -> usize {
        self.seen
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .len()
    }

    /// A snapshot of the requests the provider received.
    pub fn requests(&self) -> Vec<ModelRequest> {
        self.seen
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }

    fn next(&self) -> Reply {
        let mut replies = self.replies.lock().unwrap_or_else(|held| held.into_inner());
        match replies.len() {
            0 => Reply::Events(Vec::new()),
            1 => replies[0].clone(),
            _ => replies
                .pop_front()
                .unwrap_or(Reply::Events(Vec::new()))
                .clone(),
        }
    }
}

impl Provider for FakeProvider {
    async fn stream(&self, request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        self.seen
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(request);
        match self.next() {
            Reply::Fail(err) => Err(err),
            Reply::Events(events) => Ok(boxed(stream::iter(events))),
        }
    }
}

pub fn usage(input: u64, output: u64) -> Usage {
    Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

/// Usage with the reasoning bucket set, for the mapping rows.
pub fn reasoning_usage(input: u64, output: u64, reasoning: u64) -> Usage {
    Usage {
        reasoning_tokens: reasoning,
        ..usage(input, output)
    }
}

/// A turn that streams text and finishes the turn.
pub fn text_reply(text: &str, used: Usage) -> Reply {
    Reply::Events(vec![
        Ok(ModelEvent::TextDelta {
            text: text.to_string(),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::EndTurn,
            usage: used,
        }),
    ])
}

/// A stream that dies mid-turn after partial text: replayable.
pub fn dead_partial_reply(text: &str) -> Reply {
    Reply::Events(vec![
        Ok(ModelEvent::TextDelta {
            text: text.to_string(),
        }),
        Err(ProviderError::StreamClosed),
    ])
}

/// A turn that requests one tool call and finishes for tool use.
pub fn tool_reply(id: &str, name: &str, args: &str, used: Usage) -> Reply {
    Reply::Events(vec![
        Ok(ModelEvent::ToolCallDelta {
            index: 0,
            id: Some(id.to_string()),
            name: Some(name.to_string()),
            arguments: args.to_string(),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: used,
        }),
    ])
}

/// A unique scratch directory per test row.
pub fn scratch(name: &str) -> io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "craft-app-workflow-{}-{name}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A minimal repo root: a `.git` marker, an agents directory, and a
/// workflows directory.
pub fn repo(name: &str) -> io::Result<PathBuf> {
    let dir = scratch(name)?;
    fs::write(dir.join(".git"), "gitdir: placeholder\n")?;
    fs::create_dir_all(dir.join(".craft-agent").join("agents"))?;
    fs::create_dir_all(dir.join(".craft-agent").join("workflows"))?;
    Ok(dir)
}

pub const AGENT_NAME: &str = "step-agent";

/// Writes the step-agent profile the corpus steps name.
pub fn write_step_agent(workspace_root: &Path) -> io::Result<()> {
    let body = r#"---
name: step-agent
description: >-
  Use when a workflow step needs a bounded worker for one task.
  Runs the step body and reports in the final message.
promptMode: full
tools: read_file
---
You are a workflow step worker. Do the step body and report the
outcome in your final message.
"#;
    fs::write(
        workspace_root
            .join(".craft-agent")
            .join("agents")
            .join("step-agent.md"),
        body,
    )
}

/// The same profile as a loaded value, without touching the disk.
pub fn step_agent_profile() -> AgentFile {
    AgentFile {
        path: PathBuf::from("step-agent.md"),
        name: AGENT_NAME.to_string(),
        description: "Use for workflow steps.".to_string(),
        model: None,
        prompt_mode: PromptMode::Full,
        tools: Some(vec!["read_file".to_string()]),
        disallowed_tools: Vec::new(),
        deny_read: Vec::new(),
        deny_write: Vec::new(),
        max_turns: None,
        timeout_secs: None,
        max_context_tokens: None,
        max_input_tokens: None,
        max_output_tokens: None,
        thinking: None,
        background: false,
        agents_md: true,
        body: "You are a workflow step worker.".to_string(),
    }
}

/// The one-step plan the corpus locates, converts, and runs.
pub const TINY_PLAN: &str = r#"---
name: tiny-flow
args:
  window: { type: string }
  repos: { type: array }
steps:
  - id: only
    agent: step-agent
    body: |
      Do the one step.
---

Shared rules.
"#;

/// Parses argv after `craft-agent workflow` into the typed config.
pub fn parse_workflow_cli(args: &[&str]) -> Result<craft_app::cli::WorkflowCli, clap::Error> {
    let mut argv = vec!["craft-agent", "workflow"];
    argv.extend_from_slice(args);
    let cli = craft_app::cli::parse_cli(argv)?;
    match cli.command {
        craft_app::cli::CliCommand::Workflow(workflow) => Ok(workflow),
        _ => Err(clap::Error::raw(
            clap::error::ErrorKind::InvalidSubcommand,
            "expected the workflow subcommand",
        )),
    }
}

/// A flat env map from pairs.
pub fn env_map(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

/// Writes one plan file under `dir`; the caller asserts the write.
pub fn plan_file(dir: &Path, body: &str) -> std::io::Result<PathBuf> {
    let path = dir.join(format!("plan-{}.md", std::process::id()));
    fs::write(&path, body)?;
    Ok(path)
}
