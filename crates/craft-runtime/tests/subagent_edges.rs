//! Edge corpus: the recursion bound (the depth-2 registry carries no
//! spawn pair; deeper admission is a typed refusal), the wall-clock
//! timeout, cross-child clobber detection, and stationarity — spawn
//! calls participate in the parent guard while child-internal repeats
//! never touch it.

mod subagent_common;

use std::time::Duration;

use craft_runtime::AdmitBatch;
use craft_runtime::ExitStatus;
use craft_runtime::SubagentCaps;

use subagent_common::{Reply, delayed, text_reply, tool_reply, usage};

/// One admission's batch shape, in compact call form.
fn batch(slice: u64, count: u32) -> AdmitBatch {
    AdmitBatch {
        slice: Some(slice),
        count,
    }
}

#[tokio::test]
async fn depth_cap_removes_the_spawn_pair_at_the_ceiling() {
    let provider = subagent_common::FakeProvider::new(vec![
        tool_reply(
            "c1",
            "spawn_agent",
            "{\"agent\": \"pr-analyst\", \"prompt\": \"delegate the scan\", \"maxTokens\": 2000}",
            usage(10, 5),
        ),
        tool_reply(
            "k1",
            "spawn_agent",
            "{\"agent\": \"repo-scout\", \"prompt\": \"grandchild task\"}",
            usage(5, 5),
        ),
        text_reply("grand done", usage(5, 5)),
        text_reply("child done", usage(5, 5)),
        text_reply("parent done", usage(10, 5)),
    ]);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("depth-cap", 100_000),
        provider.clone(),
        vec![subagent_common::scout(), subagent_common::spawner()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider.clone(), "Drive the run.").await;
    assert_eq!(outcome.exit, ExitStatus::Ok, "{:?}", outcome.detail);
    let requests = provider.requests();
    let names = |request: &craft_core::types::ModelRequest| -> Vec<String> {
        request.tools.iter().map(|tool| tool.name.clone()).collect()
    };
    assert!(
        names(&requests[1]).contains(&String::from("spawn_agent")),
        "the depth-1 child carries the pair: {:?}",
        names(&requests[1])
    );
    let grand_tools = names(&requests[2]);
    assert!(
        !grand_tools.contains(&String::from("spawn_agent"))
            && !grand_tools.contains(&String::from("spawn_agents")),
        "the depth-2 registry has no spawn tools: {grand_tools:?}"
    );
    assert_eq!(
        subagent_common::started(&events),
        vec![
            ("agent-1".into(), "pr-analyst".into()),
            ("agent-2".into(), "repo-scout".into()),
        ]
    );
    let shallow = subagent_common::rig(
        subagent_common::RigSpec::memory("depth-refusal", 100_000).caps(SubagentCaps {
            max_depth: 1,
            ..SubagentCaps::default()
        }),
        subagent_common::FakeProvider::new(vec![]),
        vec![subagent_common::scout()],
    );
    let refusal = shallow
        .hub
        .admit(
            1,
            batch(100, 1),
            "repo-scout",
            craft_runtime::PoolFull::Refuse,
        )
        .expect_err("a spawn from depth 1 under a depth-1 cap is refused");
    assert!(refusal.starts_with("depth:"), "{refusal}");
}

#[tokio::test(start_paused = true)]
async fn wall_clock_expiry_settles_timed_out_and_the_parent_continues() {
    let provider = subagent_common::FakeProvider::new(vec![
        tool_reply(
            "c1",
            "spawn_agent",
            "{\"agent\": \"slowpoke\", \"prompt\": \"take your time\"}",
            usage(10, 5),
        ),
        delayed(
            text_reply("never lands", usage(1, 1)),
            Duration::from_secs(2),
        ),
        text_reply("parent done", usage(10, 5)),
    ]);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("timeout", 100_000),
        provider.clone(),
        vec![subagent_common::slowpoke()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider, "Drive the run.").await;
    assert_eq!(outcome.exit, ExitStatus::Ok, "{:?}", outcome.detail);
    let finished = subagent_common::finished(&events);
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].1, "timed_out");
    let results = subagent_common::tool_results(&events);
    assert!(results[0].0);
    assert!(
        results[0].1.contains("exceeded its wall-clock cap (1s)"),
        "the typed timeout detail: {}",
        results[0].1
    );
}

/// One member's replies: a read of the shared file, then an edit that
/// writes `member` over the base word.
fn clobber_replies(delay_first: bool) -> Vec<Reply> {
    let spawn = tool_reply(
        "c1",
        "spawn_agents",
        "{\"agent\": \"docs-writer\", \"promptTemplate\": \"edit for {{{{item}}}}\", \"items\": [\"one\", \"two\"]}",
        usage(10, 5),
    );
    let read = |id: &str| tool_reply(id, "read_file", "{\"path\": \"shared.txt\"}", usage(1, 1));
    let edit = |id: &str, member: &str| {
        let args = format!(
            "{{\"path\": \"shared.txt\", \"old_string\": \"alpha\", \"new_string\": \"{member}\"}}"
        );
        tool_reply(id, "edit_file", &args, usage(1, 1))
    };
    let delayed_edit = |id: &str, member: &str| {
        let args = format!(
            "{{\"path\": \"shared.txt\", \"old_string\": \"alpha\", \"new_string\": \"{member}\"}}"
        );
        delayed(
            tool_reply(id, "edit_file", &args, usage(1, 1)),
            Duration::from_millis(20),
        )
    };
    let one_done = text_reply("member one done", usage(1, 1));
    let two_done = text_reply("member two done", usage(1, 1));
    let parent_done = text_reply("parent done", usage(10, 5));
    if delay_first {
        // Member one stalls on its edit while member two completes.
        vec![
            spawn,
            read("a1"),
            delayed_edit("e1", "one"),
            read("a2"),
            edit("e2", "two"),
            two_done,
            one_done,
            parent_done,
        ]
    } else {
        // Member one completes before member two's second turn.
        vec![
            spawn,
            read("a1"),
            edit("e1", "one"),
            one_done,
            read("a2"),
            delayed_edit("e2", "two"),
            two_done,
            parent_done,
        ]
    }
}

/// The delayed member's child reports the typed clobber: it read before
/// or after the winner's write, so the refusal is the stale-stamp or
/// the stale-content form.
fn assert_loser_refused(transcript: &str) {
    let refused =
        transcript.contains("changed on disk") || transcript.contains("old_string not found");
    assert!(
        refused,
        "the delayed member's child reports the typed clobber"
    );
}

/// One clobber row: two members race one file; only the delayed
/// member's edit fails typed. The queue pins the winner: the
/// un-delayed member's replies all sit ahead of the delayed member's
/// second turn, so the loser retries into terminal text and can
/// never apply the winner's words.
#[allow(clippy::expect_used)]
async fn clobber_row(delay_first: bool) {
    let provider = subagent_common::FakeProvider::new(clobber_replies(delay_first));
    let winner = if delay_first { "two" } else { "one" };
    let loser = if delay_first { "agent-1" } else { "agent-2" };
    let rig = subagent_common::rig(
        subagent_common::RigSpec::logged("clobber", 100_000),
        provider.clone(),
        vec![subagent_common::writer()],
    );
    std::fs::write(rig.workspace.join("shared.txt"), "alpha\n").expect("base file");
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider, "Drive the run.").await;
    assert_eq!(outcome.exit, ExitStatus::Ok, "{:?}", outcome.detail);
    let content = std::fs::read_to_string(rig.workspace.join("shared.txt")).expect("read back");
    assert_eq!(
        content,
        format!("{winner}\n"),
        "the un-delayed member's edit commits first and survives"
    );
    let transcript_path = rig
        .workspace
        .join("sessions")
        .join("parent")
        .join("agents")
        .join(loser)
        .join("session.jsonl");
    let transcript = std::fs::read_to_string(&transcript_path).expect("loser transcript");
    assert_loser_refused(&transcript);
    assert!(
        subagent_common::tool_results(&events)[0]
            .1
            .contains("tally: completed: 2, failed: 0, timed_out: 0")
    );
}

#[tokio::test]
async fn cross_child_clobber_fails_the_second_editor_typed() {
    clobber_row(true).await;
}

#[tokio::test]
async fn cross_child_clobber_covering_the_swapped_delay_order() {
    clobber_row(false).await;
}

#[tokio::test]
async fn identical_parent_spawns_stop_the_parent_run() {
    let args = "{\"agent\": \"repo-scout\", \"prompt\": \"same task\"}";
    let mut replies = Vec::new();
    for _ in 0..8 {
        replies.push(tool_reply("c1", "spawn_agent", args, usage(1, 1)));
        replies.push(text_reply("child done", usage(1, 1)));
    }
    let provider = subagent_common::FakeProvider::new(replies);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("stationary-parent", 100_000),
        provider.clone(),
        vec![subagent_common::scout()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 40);
    let (outcome, events) =
        subagent_common::run(config, registry, provider, "Drive the run.").await;
    assert_eq!(
        outcome.exit,
        ExitStatus::Stationarity,
        "eight identical spawn calls stop the parent: {:?}",
        outcome.detail
    );
    assert_eq!(
        subagent_common::started(&events).len(),
        8,
        "the children ran; only the parent's calls repeated"
    );
}

#[tokio::test]
async fn child_internal_repeats_never_touch_the_parent_guard() {
    let mut replies = vec![tool_reply(
        "c1",
        "spawn_agent",
        "{\"agent\": \"repo-scout\", \"prompt\": \"loop the read\"}",
        usage(1, 1),
    )];
    for i in 0..8 {
        replies.push(tool_reply(
            &format!("k{i}"),
            "read_file",
            "{\"path\": \"missing.txt\"}",
            usage(1, 1),
        ));
    }
    replies.push(text_reply("parent done", usage(1, 1)));
    let provider = subagent_common::FakeProvider::new(replies);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("stationary-child", 100_000),
        provider.clone(),
        vec![subagent_common::scout()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider, "Drive the run.").await;
    assert_eq!(
        outcome.exit,
        ExitStatus::Ok,
        "the parent completes while its child went stationary: {:?}",
        outcome.detail
    );
    let finished = subagent_common::finished(&events);
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].1, "stationarity");
    assert_eq!(outcome.turns, 2, "the parent needed only its two turns");
}
