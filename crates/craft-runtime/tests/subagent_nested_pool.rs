//! Nested-pool corpus: a detached spawn under a saturated pool queues
//! instead of refusing, a running child's continuation launches ahead
//! of fresh dispatches the root parent has not started, and the
//! blocking path still refuses so its awaiting caller is never hung.

mod subagent_common;

use std::time::Duration;

use craft_runtime::ExitStatus;

use subagent_common::{delayed, text_reply, tool_reply, usage};

/// One detached batch call.
fn detached_args(items: &[&str], extra: &str) -> String {
    let list = items
        .iter()
        .map(|item| format!("\"{item}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"agent\": \"repo-scout\", \"promptTemplate\": \"report on {{{{item}}}}\", \"items\": [{list}]{extra}}}"
    )
}

#[tokio::test]
async fn a_saturated_pool_queues_a_detached_spawn_instead_of_refusing() {
    // Cap 1: the first call launches one member and queues the rest;
    // the second call finds the pool saturated and must queue, not
    // refuse — the caller already returned at admission.
    let first = detached_args(&["alpha", "beta", "gamma"], ", \"detached\": true");
    let second = "{\"agent\": \"repo-scout\", \"prompt\": \"report on delta\", \"detached\": true}";
    let provider = subagent_common::FakeProvider::new(vec![
        tool_reply("c1", "spawn_agents", &first, usage(10, 5)),
        tool_reply("c2", "spawn_agent", second, usage(10, 5)),
        tool_reply("c3", "poll_agents", "{\"waitSecs\": 10}", usage(10, 5)),
        text_reply("scout alpha done", usage(4, 2)),
        text_reply("scout beta done", usage(4, 2)),
        text_reply("scout gamma done", usage(4, 2)),
        text_reply("scout delta done", usage(4, 2)),
        text_reply("parent done", usage(10, 5)),
    ]);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("detached-saturated-queue", 100_000).caps(
            craft_runtime::SubagentCaps {
                max_depth: 2,
                max_total: 32,
                max_concurrency: 1,
            },
        ),
        provider.clone(),
        vec![subagent_common::scout()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider.clone(), "Drive the run.").await;
    assert_eq!(outcome.exit, ExitStatus::Ok, "{:?}", outcome.detail);

    let results = subagent_common::tool_results(&events);
    // Both calls were accepted; the second one queued while the pool
    // was full instead of answering with a typed refusal.
    assert!(
        results[0].1.contains("detached: 3 accepted"),
        "{}",
        results[0].1
    );
    assert!(!results[0].0, "the first call is not an error");
    assert!(
        results[1].1.contains("detached: 1 accepted"),
        "{}",
        results[1].1
    );
    assert!(
        !results[1].0,
        "a saturated pool queues the detached call: {}",
        results[1].1
    );
    // Every member ran under the cap and settled, and the poll
    // collected every envelope.
    assert!(
        results[2].1.contains("tally: completed: 4"),
        "{}",
        results[2].1
    );
    for id in ["agent-1", "agent-2", "agent-3", "agent-4"] {
        assert!(
            results[2].1.contains(&format!("agent_id: {id}")),
            "{id} never collected: {}",
            results[2].1
        );
    }
    assert_eq!(
        subagent_common::started(&events).len(),
        4,
        "four children launched"
    );
    assert_eq!(
        subagent_common::finished(&events).len(),
        4,
        "four children settled"
    );
    assert!(
        provider.max_concurrency() <= 1,
        "the pool never exceeded the cap; saw {}",
        provider.max_concurrency()
    );
}

#[tokio::test]
async fn a_nested_child_spawn_queues_instead_of_refusing() {
    // The parent holds the only slot with a pipeline child; that child
    // dispatches its own stage child while the pool is saturated. The
    // stage child must queue and launch once the pipeline settles.
    let pipeline_spawn =
        "{\"agent\": \"pr-pipeline\", \"prompt\": \"run the alpha pipeline\", \"detached\": true}";
    let stage_spawn =
        "{\"agent\": \"repo-scout\", \"prompt\": \"report on stage\", \"detached\": true}";
    let provider = subagent_common::FakeProvider::new(vec![
        tool_reply("c1", "spawn_agent", pipeline_spawn, usage(10, 5)),
        tool_reply("c2", "poll_agents", "{\"waitSecs\": 10}", usage(10, 5)),
        tool_reply("p1", "spawn_agent", stage_spawn, usage(10, 5)),
        tool_reply(
            "p2",
            "poll_agents",
            "{\"ids\": [\"agent-2\"], \"waitSecs\": 2}",
            usage(10, 5),
        ),
        text_reply("pipeline done", usage(10, 5)),
        text_reply("scout stage done", usage(4, 2)),
        text_reply("parent done", usage(10, 5)),
    ]);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("detached-nested-queue", 100_000).caps(
            craft_runtime::SubagentCaps {
                max_depth: 2,
                max_total: 32,
                max_concurrency: 1,
            },
        ),
        provider.clone(),
        vec![subagent_common::pipeline(), subagent_common::scout()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider.clone(), "Drive the run.").await;
    assert_eq!(outcome.exit, ExitStatus::Ok, "{:?}", outcome.detail);

    let results = subagent_common::tool_results(&events);
    // The parent's own calls settled cleanly: no refusal anywhere.
    for (is_error, text) in &results {
        assert!(!is_error, "a tool result carries an error: {text}");
    }
    let started = subagent_common::started(&events);
    assert_eq!(
        started,
        vec![
            (String::from("agent-1"), String::from("pr-pipeline")),
            (String::from("agent-2"), String::from("repo-scout")),
        ],
        "the stage child waited for the pipeline's slot, then launched"
    );
    assert_eq!(subagent_common::finished(&events).len(), 2);
}

#[tokio::test]
async fn a_continuation_launches_ahead_of_fresh_work() {
    // Cap 1: two fresh pipelines are dispatched in one call — the
    // second queues. The first, running, dispatches its own stage
    // child, admitted third. The continuation must launch before the
    // fresh pipeline admitted earlier: a PR's next stage never waits
    // behind a pipeline that has not begun.
    let first = "{\"agent\": \"pr-pipeline\", \"promptTemplate\": \"run the pipeline for {{{{{{item}}}}}}\", \"items\": [\"pipeline-one\", \"pipeline-two\"], \"detached\": true}";
    let stage_spawn =
        "{\"agent\": \"repo-scout\", \"prompt\": \"report on stage\", \"detached\": true}";
    let provider = subagent_common::FakeProvider::new(vec![
        tool_reply("c1", "spawn_agents", first, usage(10, 5)),
        tool_reply("c2", "poll_agents", "{\"waitSecs\": 30}", usage(10, 5)),
        tool_reply("p1", "spawn_agent", stage_spawn, usage(10, 5)),
        tool_reply(
            "p2",
            "poll_agents",
            "{\"ids\": [\"agent-3\"], \"waitSecs\": 2}",
            usage(10, 5),
        ),
        text_reply("pipeline done", usage(10, 5)),
        text_reply("scout stage done", usage(4, 2)),
        text_reply("pipeline two done", usage(10, 5)),
        text_reply("parent done", usage(10, 5)),
    ]);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("detached-continuation-priority", 100_000).caps(
            craft_runtime::SubagentCaps {
                max_depth: 2,
                max_total: 32,
                max_concurrency: 1,
            },
        ),
        provider.clone(),
        vec![subagent_common::pipeline(), subagent_common::scout()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider.clone(), "Drive the run.").await;
    assert_eq!(outcome.exit, ExitStatus::Ok, "{:?}", outcome.detail);

    // Admission order: agent-1 fresh, agent-2 fresh, agent-3 the
    // continuation. Launch order must be 1, 3, 2.
    let started: Vec<String> = subagent_common::started(&events)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        started,
        vec![
            String::from("agent-1"),
            String::from("agent-3"),
            String::from("agent-2")
        ],
        "the continuation launched before the fresh sibling admitted earlier"
    );
    assert_eq!(subagent_common::finished(&events).len(), 3);
}

#[tokio::test]
async fn a_blocking_spawn_still_refuses_when_the_pool_is_full() {
    // A detached child holds the only slot. The blocking path must
    // still refuse: its caller awaits the whole wave, so a queue
    // would hang the call.
    let hold = "{\"agent\": \"repo-scout\", \"prompt\": \"report on hold\", \"detached\": true}";
    let blocking = "{\"agent\": \"repo-scout\", \"prompt\": \"report on beta\"}";
    let provider = subagent_common::FakeProvider::new(vec![
        tool_reply("c1", "spawn_agent", hold, usage(10, 5)),
        tool_reply("c2", "spawn_agent", blocking, usage(10, 5)),
        tool_reply("c3", "poll_agents", "{\"waitSecs\": 10}", usage(10, 5)),
        delayed(
            text_reply("scout hold done", usage(4, 2)),
            Duration::from_secs(1),
        ),
        text_reply("parent done", usage(10, 5)),
    ]);
    let rig = subagent_common::rig(
        subagent_common::RigSpec::memory("detached-blocking-refuses", 100_000).caps(
            craft_runtime::SubagentCaps {
                max_depth: 2,
                max_total: 32,
                max_concurrency: 1,
            },
        ),
        provider.clone(),
        vec![subagent_common::scout()],
    );
    let (config, registry) = subagent_common::parent_run(&rig, 20);
    let (outcome, events) =
        subagent_common::run(config, registry, provider.clone(), "Drive the run.").await;
    assert_eq!(outcome.exit, ExitStatus::Ok, "{:?}", outcome.detail);

    let results = subagent_common::tool_results(&events);
    assert!(results[1].0, "the blocking call is a typed error");
    assert!(
        results[1].1.contains("concurrency:"),
        "the refusal names the saturated pool: {}",
        results[1].1
    );
    // The detached holder still ran to completion.
    assert!(results[2].1.contains("agent-1"), "{}", results[2].1);
    assert!(
        results[2].1.contains("status: completed"),
        "{}",
        results[2].1
    );
    assert_eq!(
        subagent_common::started(&events).len(),
        1,
        "only the detached child launched"
    );
}
