//! The scripted model the simulation drives: one Provider that routes on
//! the prompt marker and replays the per-PR-life architecture — the
//! orchestrator's one-turn detached dispatch of per-PR pipelines, each
//! pipeline spawning its own stage children while its siblings are
//! still early, the filing lease before any write, the per-PR rollup
//! sections, and the seal. Polls carry explicit ids only: the hub is
//! shared, so a bulk drain would steal a sibling's envelope.
//!
//! Two routing keys, because a run's turns come in two shapes: a first
//! turn's last message is the user prompt, and every later turn's last
//! message is a tool result. The first-turn key dispatches; the
//! originating-prompt key — the conversation's own user message — routes
//! each continuation back to whoever owns it.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use craft_core::error::ProviderError;
use craft_core::events::ModelEvent;
use craft_core::provider::{ModelEventStream, Provider, boxed};
use craft_core::types::{ChatMessage, ModelRequest, StopReason, Usage};
use futures::stream;

mod life;

use life::{
    PRS, accepted_ids, actionable, emit_committer, emit_lease, emit_section, emit_stage_two,
    life_shard, outstanding_ids, poll_call, verdict_line_for,
};

/// The scripted model: routes on the user prompt's marker, records every
/// request, and counts how many malformed judge verdicts it emitted (the
/// seat-repair proof).
pub struct SimProvider {
    seen: Mutex<Vec<ModelRequest>>,
    malformed: AtomicU32,
}

impl SimProvider {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            seen: Mutex::new(Vec::new()),
            malformed: AtomicU32::new(0),
        })
    }

    pub fn calls(&self) -> usize {
        self.seen.lock().expect("seen").len()
    }

    pub fn malformed(&self) -> u32 {
        self.malformed.load(Ordering::Relaxed)
    }

    /// The last user message in full — the first-turn routing key: a
    /// first turn's last message IS the prompt.
    fn prompt_of(request: &ModelRequest) -> String {
        request
            .messages
            .last()
            .map(|message| match message.as_ref() {
                ChatMessage::User { text, .. } => text.clone(),
                _ => String::new(),
            })
            .unwrap_or_default()
    }

    /// The conversation's own user message — the continuation routing
    /// key: after a tool call the last message is the tool result, so
    /// the speaking agent is recovered from what started it.
    fn origin_of(request: &ModelRequest) -> String {
        request
            .messages
            .iter()
            .rev()
            .find_map(|message| match message.as_ref() {
                ChatMessage::User { text, .. } => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The last tool result's content — the continuation's shape key.
    fn tool_result_of(request: &ModelRequest) -> String {
        request
            .messages
            .iter()
            .rev()
            .find_map(|message| match message.as_ref() {
                ChatMessage::Tool { content, .. } => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }
}

impl Provider for SimProvider {
    async fn stream(&self, request: ModelRequest) -> Result<ModelEventStream, ProviderError> {
        self.seen.lock().expect("seen").push(request.clone());
        let prompt = Self::prompt_of(&request);
        let origin = Self::origin_of(&request);
        let tool_result = Self::tool_result_of(&request);
        let events = route(&prompt, &origin, &tool_result, &self.malformed);
        Ok(boxed(stream::iter(events)))
    }
}

fn usage() -> Usage {
    Usage {
        input_tokens: 3,
        output_tokens: 1,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

fn text(text: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    vec![
        Ok(ModelEvent::TextDelta {
            text: text.to_string(),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::EndTurn,
            usage: usage(),
        }),
    ]
}

fn call(id: &str, name: &str, args: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    vec![
        Ok(ModelEvent::ToolCallDelta {
            index: 0,
            id: Some(id.to_string()),
            name: Some(name.to_string()),
            arguments: args.to_string(),
        }),
        Ok(ModelEvent::TurnFinished {
            stop_reason: StopReason::ToolUse,
            usage: usage(),
        }),
    ]
}

/// The scripted model's behaviour per marker. Every spawn call carries
/// `maxTurns: 10000`, a finite `timeoutSecs`, and omits `maxTokens`.
fn route(
    prompt: &str,
    origin: &str,
    tool_result: &str,
    malformed: &AtomicU32,
) -> Vec<Result<ModelEvent, ProviderError>> {
    if tool_result.is_empty() {
        // FIRST TURNS — the prompt is the last message, and it dispatches.
        if prompt.starts_with("LIFE ") {
            // One per-PR pipeline child: stage 1 is its own classifier,
            // spawned detached.
            let shard = life_shard(prompt);
            call(
                "spawn-classifier",
                "spawn_agent",
                &format!(
                    r#"{{"agent":"sim-classifier","prompt":"PR {shard} — read your shard and write your verdict file","maxTurns":10000,"timeoutSecs":3600,"detached":true}}"#
                ),
            )
        } else if prompt.starts_with("PR ") {
            // The pipeline's classifier stage child: write the verdict
            // file, whose first line is the verdict.
            let shard = prompt.trim_start_matches("PR ").trim();
            let number = shard.split('|').next().unwrap_or("x").to_string();
            let class = shard.split('|').nth(1).unwrap_or("irrelevant");
            call(
                "verdict-write",
                "write_file",
                &format!(
                    r#"{{"path":"run-ledger/classifier/{}.verdict","content":"{} | {} | scripted reason | files: {}\n"}}"#,
                    number,
                    class,
                    number,
                    shard.split('|').nth(2).unwrap_or("none")
                ),
            )
        } else if prompt.starts_with("STAGE2 ") {
            // The pipeline's stage-2 drafter child: the draft package.
            let slug = prompt
                .trim_start_matches("STAGE2 ")
                .split(" — ")
                .next()
                .unwrap_or("x")
                .split('|')
                .next()
                .unwrap_or("x")
                .trim()
                .to_string();
            call(
                "draft-write",
                "write_file",
                &format!(
                    r#"{{"path":"run-ledger/drafts/{}.md","content":"draft with evidence\n"}}"#,
                    slug
                ),
            )
        } else if prompt.starts_with("COMMIT ") {
            // The pipeline's committer stage child: the outcome row,
            // after the pipeline took its filing lease.
            let slug = prompt.trim_start_matches("COMMIT ").trim().to_string();
            call(
                "outcome-write",
                "write_file",
                &format!(
                    r#"{{"path":"run-ledger/outcomes/file-{}.md","content":"filed #1\n"}}"#,
                    slug
                ),
            )
        } else if prompt.starts_with("ROLLUP ") {
            call(
                "rollup-write",
                "write_file",
                r#"{"path":"run-ledger/rollup/state-body.md","content":"matched\n"}"#,
            )
        } else if origin_or_prompt_has_gate(prompt, origin) {
            // A judge seat. Seat 1 of the pipeline gate first answers in
            // prose so the engine's seat-repair loop must correct it; the
            // repair round carries the parser's complaint and gets JSON.
            if prompt.contains("[seat 1 of 3]")
                && prompt.contains("GATE:pipeline")
                && !prompt.contains("Verdict format repair")
            {
                malformed.fetch_add(1, Ordering::Relaxed);
                text("The pipeline looks complete to me; I saw the files and they matched.")
            } else {
                text(r#"{"verdict":"pass","issues":[]}"#)
            }
        } else if prompt.contains("SIM SWEEP") {
            // The scripted sweep: one bash call records the three
            // artifacts the launch would have produced.
            call(
                "sweep-bash",
                "bash",
                r#"{"command":"mkdir -p run-ledger/prepare run-ledger/classifier run-ledger/drafts run-ledger/outcomes run-ledger/rollup run-ledger/filing run-ledger/pipeline && : > run-ledger/prepare/.collect-done && : > run-ledger/prepare/.sweep-done && : > run-ledger/prepare/.digest-done && date -u +%Y-%m-%dT%H:%M:%SZ | tee run-ledger/prepare/.collect-done"}"#,
            )
        } else if prompt.contains("SIM PIPELINE") {
            // The per-PR fan-out: ONE detached call for every PR,
            // returning at admission — no wave is awaited, and each
            // pipeline owns its PR's whole life from here.
            let items = PRS
                .iter()
                .map(|shard| format!("\"{shard}\""))
                .collect::<Vec<_>>()
                .join(", ");
            call(
                "spawn-prs",
                "spawn_agents",
                &format!(
                    r#"{{"agent":"sim-pipeline","promptTemplate":"LIFE {{{{item}}}} — run this PR's whole life","items":[{items}],"maxTurns":10000,"timeoutSecs":3600,"detached":true}}"#
                ),
            )
        } else if prompt.contains("SIM SEAL") {
            call(
                "spawn-rollup",
                "spawn_agent",
                r#"{"agent":"sim-rollup","prompt":"ROLLUP state — compose the drafts from the per-PR sections","maxTurns":10000,"timeoutSecs":1800}"#,
            )
        } else if prompt.contains("SIM COMMIT") {
            text("commit recorded: run-ledger/outcomes/rollup.md")
        } else {
            text("done")
        }
    } else if origin.starts_with("LIFE ") {
        // A PIPELINE's continuation — its own PR's life, one stage at a
        // time, decided by this PR alone and never by a sibling.
        let shard = life_shard(origin);
        if tool_result.starts_with("detached:") {
            // The spawn returned at admission: poll ONLY the ids this
            // acceptance named — a bulk drain would steal a sibling's
            // envelope.
            poll_call(&accepted_ids(tool_result), 10)
        } else if tool_result.contains("agent_id:")
            && tool_result.contains("profile: sim-classifier")
        {
            // This PR's classifier envelope landed. An actionable PR
            // gets its stage-2 drafter; an irrelevant one its own
            // rollup section. Neither waits on a sibling.
            if actionable(&shard) {
                emit_stage_two(&shard)
            } else {
                emit_section(&shard)
            }
        } else if tool_result.contains("agent_id:")
            && (tool_result.contains("profile: sim-applier")
                || tool_result.contains("profile: sim-filer"))
        {
            // The drafter's package is written and its envelope landed:
            // the lease comes BEFORE any committer.
            emit_lease(&shard)
        } else if tool_result.contains("agent_id:")
            && (tool_result.contains("profile: sim-filing-committer")
                || tool_result.contains("profile: sim-bump-committer"))
        {
            // The write landed: this PR's own rollup section.
            emit_section(&shard)
        } else if tool_result.contains("agent_id:") {
            // The section write's envelope: the PR's life is complete.
            text(&format!("life complete: {shard}"))
        } else if tool_result.contains("leased run-ledger/filing/") {
            // The lease held: the committer performs the write.
            emit_committer(&shard)
        } else if tool_result.contains("run-ledger/rollup/") {
            // The section write landed: the PR's life is complete.
            text(&format!("life complete: {shard}"))
        } else {
            text("done")
        }
    } else if origin.starts_with("PR ") {
        // The classifier's continuation: its write landed, so the
        // envelope carries the verdict line.
        text(&verdict_line_for(tool_result))
    } else if origin.starts_with("STAGE2 ") {
        text("package written")
    } else if origin.starts_with("COMMIT ") {
        text("filed #1")
    } else if origin.starts_with("ROLLUP ") {
        text("drafts composed")
    } else if origin_or_prompt_has_gate(prompt, origin) {
        text(r#"{"verdict":"pass","issues":[]}"#)
    } else {
        // The orchestrator's continuations: poll by the ids its own
        // spawn named, and finish only when nothing is outstanding.
        if tool_result.starts_with("detached:") {
            poll_call(&accepted_ids(tool_result), 10)
        } else if tool_result.contains("agent_id:") || tool_result.contains("outstanding:") {
            if tool_result.contains("outstanding: none") {
                text("per-PR pipeline complete: every PR's journey ended")
            } else {
                poll_call(&outstanding_ids(tool_result), 10)
            }
        } else {
            text("done")
        }
    }
}

/// A judge marker in either key: the seat's own prompt, or the
/// conversation's origin when the repair turn's last message is the
/// parser's complaint.
fn origin_or_prompt_has_gate(prompt: &str, origin: &str) -> bool {
    prompt.contains("GATE:") || origin.contains("GATE:")
}
