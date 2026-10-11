//! Live agent-loop corpus: one real BYOK run through the Runner with a
//! single deterministic tool in the registry. Without the TEST_BYOK_* trio
//! in the repo-root .env the corpus prints its SKIP reason and passes.

#[path = "../../craft-providers/tests/byok_env.rs"]
mod byok_env;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use craft_core::types::ChatMessage;
use craft_providers::backend::Backend;
use craft_providers::openai::OpenAiProvider;
use craft_runtime::{CancelCategory, RunConfig, Runner, RuntimeEvent};
use craft_tools::bash::BashTool;
use craft_tools::{Registry, ToolLimits};

type TestError = Box<dyn Error + Send + Sync>;

// The exact string the run must echo back; proves the tool result
// round-tripped through the loop into the final answer.
const MARKER: &str = "craft-agent-loop-live-ok";

// Bounds the whole run; a stalled gateway is a broken corpus, not a hang.
const LIVE_BOUND: Duration = Duration::from_secs(240);

// A unique workspace per run: the session log appends, so a reused path
// would mix runs and break the final-record check.
fn fresh_workspace() -> Result<PathBuf, TestError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("the clock sat before the epoch: {e}"))?;
    let root = std::env::temp_dir().join(format!(
        "craft-agent-loop-live-{}-{}",
        std::process::id(),
        nanos.as_nanos()
    ));
    std::fs::create_dir_all(&root)
        .map_err(|e| format!("the temp workspace was not created: {e}"))?;
    Ok(root)
}

#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn integration_agent_loop() -> Result<(), TestError> {
    let keys = crate::require_keys!("agent loop e2e");
    let provider = OpenAiProvider::new(&keys.base_url, &keys.model, &keys.api_key)?;
    let backend = Backend::OpenAi(provider);

    let limits = ToolLimits::default();
    let mut registry = Registry::new(limits);
    registry.register(Box::new(BashTool::new(limits)));

    let root = fresh_workspace()?;
    let session = root.join("session.jsonl");
    let mut config = RunConfig::new(root);
    config.system = Some(String::from(
        "You are a test agent. Follow instructions exactly and answer briefly.",
    ));
    config.session_path = Some(session);
    let runner = Runner::new(Arc::new(backend), registry, config);

    let prompt = format!(
        "Use the bash tool to run exactly `echo {MARKER}`, then reply with the exact command output followed by the word done."
    );
    let mut events = Vec::new();
    let outcome = tokio::time::timeout(LIVE_BOUND, runner.run(&prompt, |event| events.push(event)))
        .await
        .map_err(|_| String::from("the live run exceeded the 240s bound"))?;

    assert!(
        events
            .iter()
            .any(|e| matches!(e, RuntimeEvent::ToolCall { name, .. } if name == "bash")),
        "the model must dispatch the bash tool"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            RuntimeEvent::ToolResult { name, is_error, content, .. }
            if name == "bash" && !*is_error && content.contains(MARKER)
        )),
        "the executed tool result must carry the echoed marker"
    );
    let reply: String = events
        .iter()
        .filter_map(|e| match e {
            RuntimeEvent::AssistantDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        reply.contains(MARKER),
        "the model must fold the tool result into its final answer"
    );

    assert_eq!(
        outcome.exit_code(),
        0,
        "the run must finish clean: {}",
        outcome.detail
    );
    assert_eq!(outcome.category, CancelCategory::Completed);
    assert!(!outcome.usage.incomplete, "usage must be reported");
    assert!(
        outcome.usage.input_tokens + outcome.usage.output_tokens > 0,
        "usage must carry tokens"
    );
    assert!(
        matches!(
            events.last(),
            Some(RuntimeEvent::RunFinished { exit_code, category, .. })
            if *exit_code == 0 && *category == CancelCategory::Completed
        ),
        "the terminal event must agree with the outcome"
    );

    let session_path = outcome
        .session_path
        .clone()
        .ok_or_else(|| String::from("a session-backed run must report its log path"))?;
    let logged_text = std::fs::read_to_string(&session_path)
        .map_err(|e| format!("the session log was not readable: {e}"))?;
    let mut logged: Vec<ChatMessage> = Vec::new();
    for line in logged_text.lines().filter(|l| !l.trim().is_empty()) {
        let record: ChatMessage = serde_json::from_str(line)
            .map_err(|e| format!("a session line is not a message record: {e}"))?;
        logged.push(record);
    }
    assert!(logged.len() >= 3, "the log must carry the whole round trip");
    assert!(
        matches!(logged.last(), Some(ChatMessage::Assistant { text, .. }) if !text.trim().is_empty()),
        "the final session record must be the assistant answer"
    );

    println!(
        "live loop ok: {} turns, usage {}+{} tokens, {} session records",
        outcome.turns,
        outcome.usage.input_tokens,
        outcome.usage.output_tokens,
        logged.len()
    );
    Ok(())
}
