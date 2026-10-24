//! Hub corpus: cap-knob resolution against hostile values, the four
//! admission dimensions, joint batch reserves, exactly-once settle
//! with never-reused ids, and the child transcript layout.

mod subagent_common;

use craft_runtime::{AdmitBatch, ChildReport, PoolFull, SubagentCaps, SubagentHub};
use std::sync::Arc;

use subagent_common::{FakeProvider, Reply, RigSpec, rig, scout, text_reply, usage};

/// A completed-child report with the given input spend.
fn report(input: u64) -> ChildReport {
    ChildReport {
        stop: craft_core::subagent::ChildStop::Completed,
        usage: usage(input, 0),
        turns: 1,
        final_text: String::from("done"),
        detail: String::from("run completed"),
        session_path: None,
    }
}

#[test]
fn hostile_cap_knobs_are_typed_errors() {
    assert_eq!(
        SubagentCaps::resolve(None, None, None),
        Ok(SubagentCaps::default())
    );
    assert_eq!(
        SubagentCaps::resolve(Some("1".into()), Some("16".into()), Some("2".into())),
        Ok(SubagentCaps {
            max_depth: 1,
            max_total: 16,
            max_concurrency: 2,
        })
    );
    for hostile in ["", "  ", "-1", "no", "3.5", "99999999999999999999"] {
        assert!(
            SubagentCaps::resolve(Some(hostile.into()), None, None).is_err(),
            "{hostile:?} must be a typed error"
        );
    }
    assert!(SubagentCaps::resolve(None, Some("0".into()), None).is_err());
    assert!(SubagentCaps::resolve(None, Some("1025".into()), None).is_err());
    assert!(SubagentCaps::resolve(None, None, Some("1025".into())).is_err());
    assert!(SubagentCaps::resolve(Some("5".into()), None, None).is_err());
    assert_eq!(
        SubagentCaps::resolve(None, Some("1024".into()), None)
            .unwrap()
            .max_total,
        1024
    );
    assert_eq!(
        SubagentCaps::resolve(Some("0".into()), None, None)
            .unwrap()
            .max_depth,
        0
    );
}

fn bare_hub() -> Arc<SubagentHub> {
    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let rig = rig(RigSpec::memory("hub-bare", 10_000), provider, vec![scout()]);
    rig.hub
}

/// One admission's batch shape, in compact call form.
fn batch(slice: u64, count: u32) -> AdmitBatch {
    AdmitBatch {
        slice: Some(slice),
        count,
    }
}

fn unlimited_batch(count: u32) -> AdmitBatch {
    AdmitBatch { slice: None, count }
}

#[test]
fn admission_names_the_refusing_dimension() {
    let hub = bare_hub();
    hub.admit(0, batch(100, 1), "repo-scout", PoolFull::Refuse)
        .expect("depth 0 under every cap admits");
    let err = hub
        .admit(2, batch(100, 1), "repo-scout", PoolFull::Refuse)
        .unwrap_err();
    assert!(err.starts_with("depth:"), "{err}");
    assert!(
        hub.admit(0, batch(0, 1), "repo-scout", PoolFull::Refuse)
            .unwrap_err()
            .starts_with("token budget:")
    );
}

#[test]
fn total_concurrency_and_budget_dimensions_refuse() {
    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let total_rig = rig(
        RigSpec::memory("hub-total", 100_000).caps(SubagentCaps {
            max_total: 2,
            ..SubagentCaps::default()
        }),
        provider,
        vec![scout()],
    );
    total_rig
        .hub
        .admit(0, batch(10, 2), "repo-scout", PoolFull::Refuse)
        .expect("two members fit the total cap");
    let err = total_rig
        .hub
        .admit(0, batch(10, 1), "repo-scout", PoolFull::Refuse)
        .unwrap_err();
    assert!(err.starts_with("total:"), "{err}");

    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let conc_rig = rig(
        RigSpec::memory("hub-conc", 100_000).caps(SubagentCaps {
            max_concurrency: 1,
            ..SubagentCaps::default()
        }),
        provider,
        vec![scout()],
    );
    conc_rig
        .hub
        .admit(0, batch(10, 1), "repo-scout", PoolFull::Refuse)
        .expect("one member runs");
    conc_rig.hub.launch("agent-1", 5, None);
    let err = conc_rig
        .hub
        .admit(0, batch(10, 1), "repo-scout", PoolFull::Refuse)
        .unwrap_err();
    assert!(err.starts_with("concurrency:"), "{err}");

    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let budget_rig = rig(
        RigSpec::memory("hub-budget", 1_000),
        provider,
        vec![scout()],
    );
    budget_rig.hub.publish_parent(usage(900, 0));
    let err = budget_rig
        .hub
        .admit(0, batch(200, 1), "repo-scout", PoolFull::Refuse)
        .unwrap_err();
    assert!(err.starts_with("token budget:"), "{err}");
}

#[test]
fn batch_slices_reserve_jointly() {
    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let tight = rig(RigSpec::memory("hub-joint", 1_000), provider, vec![scout()]);
    let hub = &tight.hub;
    let ids = hub
        .admit(0, batch(400, 2), "repo-scout", PoolFull::Refuse)
        .expect("800 of 1000 fits");
    assert_eq!(ids, vec!["agent-1", "agent-2"]);
    assert!(
        hub.admit(0, batch(300, 1), "repo-scout", PoolFull::Refuse)
            .unwrap_err()
            .starts_with("token budget:")
    );
}

#[test]
fn unlimited_members_admit_past_a_tight_token_cap() {
    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let tight = rig(
        RigSpec::memory("hub-unlimited", 1_000),
        provider,
        vec![scout()],
    );
    let hub = &tight.hub;
    let ids = hub
        .admit(0, unlimited_batch(4), "repo-scout", PoolFull::Refuse)
        .expect("unlimited members reserve nothing");
    assert_eq!(ids.len(), 4);
    let remaining = hub.settle(&ids[0], &report(50));
    assert_eq!(
        remaining, 950,
        "real spend still prices against the cap: {remaining}"
    );
}

#[test]
fn settle_is_exactly_once_and_ids_never_reuse() {
    let provider = FakeProvider::new(vec![text_reply("x", usage(1, 1))]);
    let rig = rig(
        RigSpec::memory("hub-settle", 10_000),
        provider,
        vec![scout()],
    );
    let hub = &rig.hub;
    let ids = hub
        .admit(0, batch(100, 2), "repo-scout", PoolFull::Refuse)
        .expect("admitted");
    hub.launch(&ids[0], 5, None);
    let first = report(50);
    let remaining = hub.settle(&ids[0], &first);
    assert!(
        remaining < 10_000,
        "the child's spend is priced in: {remaining}"
    );
    let again = hub.settle(&ids[0], &report(50));
    assert_eq!(
        remaining, again,
        "a second settle for the same id is ignored"
    );
    let drained = hub.take_child_usage();
    assert_eq!(drained.input_tokens, 50, "exactly one fold");
    hub.launch(&ids[1], 5, None);
    hub.settle(&ids[1], &report(30));
    assert_eq!(hub.running(), 0);
    assert_eq!(hub.take_events(1).len(), 4, "two started, two finished");
    let next = hub
        .admit(0, batch(100, 1), "repo-scout", PoolFull::Refuse)
        .expect("more fit");
    assert_eq!(next, vec!["agent-3"], "ids are never reused");
}

#[test]
fn child_session_paths_derive_from_the_parent_stem() {
    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let file_rig = rig(RigSpec::logged("hub-path", 10_000), provider, vec![scout()]);
    let session = file_rig.session.clone().expect("file-backed rig");
    let child = file_rig
        .hub
        .child_session_path("agent-7")
        .expect("file-backed parents get child transcripts");
    let expected = session
        .parent()
        .unwrap()
        .join("parent")
        .join("agents")
        .join("agent-7")
        .join("session.jsonl");
    assert_eq!(child, expected);
    let provider = FakeProvider::new(vec![Reply::Events(Vec::new())]);
    let memory_rig = rig(
        RigSpec::memory("hub-path-mem", 10_000),
        provider,
        vec![scout()],
    );
    assert_eq!(memory_rig.hub.child_session_path("agent-1"), None);
}

#[test]
fn settle_of_unadmitted_or_settled_ids_returns_without_relocking() {
    let hub = bare_hub();
    let unknown = hub.settle("agent-99", &report(10));
    assert_eq!(unknown, 10_000, "an unknown id settles nothing");
    let ids = hub
        .admit(0, batch(100, 1), "repo-scout", PoolFull::Refuse)
        .expect("admitted");
    hub.launch(&ids[0], 5, None);
    hub.settle(&ids[0], &report(50));
    let again = hub.settle(&ids[0], &report(50));
    assert_eq!(again, 9_950, "a duplicate settle folds nothing more");
    hub.abandon(&ids);
    let released = hub.settle(&ids[0], &report(50));
    assert_eq!(released, again, "an abandoned id settles nothing");
}

#[test]
fn unknown_profiles_list_the_catalog() {
    let hub = bare_hub();
    let err = hub.profile("nope").unwrap_err();
    assert!(err.contains("unknown agent profile `nope`"), "{err}");
    assert!(err.contains("repo-scout (Runs when"), "{err}");
    let found = hub.profile("repo-scout").expect("the fixture resolves");
    assert_eq!(found.name, "repo-scout");
}
