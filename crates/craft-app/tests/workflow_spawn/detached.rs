//! The detached fan-out pipeline test, split out of `main` for the
//! file-length gate.

use super::*;

/// The per-item pipeline shape: a detached batch returns at admission
/// while its members run in the pool, and a later turn's poll collects
/// each envelope by id. The step therefore advances item by item
/// instead of awaiting one wave.
#[tokio::test]
async fn a_detached_fanout_returns_at_admission_and_polls_per_item() {
    let provider = FakeProvider::new(vec![
        tool_reply(
            "spawn-b",
            "spawn_agents",
            r#"{"agent":"step-agent","promptTemplate":"classify {{item}}","items":["pr-1","pr-2"],"detached":true,"timeoutSecs":60}"#,
            usage(2, 1),
        ),
        tool_reply("poll-1", "poll_agents", r#"{"waitSecs": 10}"#, usage(2, 1)),
        text_reply("pr-1 verdict", usage(1, 1)),
        text_reply("pr-2 verdict", usage(1, 1)),
        text_reply("step done", usage(1, 1)),
        text_reply("step done", usage(1, 1)),
        text_reply("step done", usage(1, 1)),
    ]);
    let rig = rig("spawn-detached").expect("rig");
    let hub = hub_over(rig.workspace.clone(), provider.clone());
    let executor =
        executor_over(&rig, provider.clone(), spawn_agent_profile()).with_subagents(Some(hub));
    let outcome = executor.execute(inputs_turns(8)).await.expect("step runs");
    assert_eq!(outcome.conclusion, StepConclusion::Success);
    assert_eq!(outcome.final_message, "step done");

    // The spawn terminal named both accepted ids without awaiting the
    // members, and the children ran as their own turns.
    let messages = transcript_texts(&rig.workspace);
    assert!(
        messages
            .iter()
            .any(|text| text.contains("detached: 2 accepted")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|text| text.contains("agent-1")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|text| text.contains("agent-2")),
        "{messages:?}"
    );
    // Every child ran, and the poll saw both settle.
    assert!(
        provider.calls() >= 4,
        "the two children ran as their own turns"
    );
}

/// The step transcript's tool-result texts, in write order.
fn transcript_texts(workspace: &std::path::Path) -> Vec<String> {
    let mut texts = Vec::new();
    let Ok(entries) = std::fs::read_dir(workspace.join("sessions")) else {
        return texts;
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    paths.sort();
    for path in paths {
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in body.lines() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(text) = value.get("content").and_then(|c| c.as_str())
            {
                texts.push(text.to_string());
            }
        }
    }
    texts
}
