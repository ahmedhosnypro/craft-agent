//! Plan validation corpus: every rule fires with its typed error.
#![allow(clippy::unwrap_used)]

mod common;

use common::no_args;
use craft_workflow::{ArgValue, Limits, OnFailure, WorkflowError, WorkflowPlan};

fn ok(text: &str) -> WorkflowPlan {
    WorkflowPlan::parse("t.md", text).unwrap()
}

fn err(text: &str) -> WorkflowError {
    WorkflowPlan::parse("t.md", text).unwrap_err()
}

const MINIMAL: &str =
    "---\nname: t\nsteps:\n  - id: a\n    agent: worker\n    body: |\n      do a\n---\n\nrules\n";

#[test]
fn parses_minimal_plan_with_defaults() {
    let plan = ok(MINIMAL);
    assert_eq!(plan.name, "t");
    assert_eq!(plan.shared_body, "rules");
    assert_eq!(plan.steps.len(), 1);
    let step = &plan.steps[0];
    assert_eq!(step.agent, "worker");
    assert_eq!(step.needs.len(), 0);
    assert_eq!(step.max_turns, 25);
    assert_eq!(step.retry, 0);
    assert_eq!(step.on_failure, OnFailure::Stop);
    assert_eq!(step.body, "do a");
    assert_eq!(
        plan.limits,
        Limits {
            max_steps: 16,
            max_total_turns: 240,
            max_total_tokens: 4_000_000,
            max_retries_per_step: 1,
            max_concurrent_steps: 1,
        }
    );
    assert_eq!(plan.plan_hash.len(), 64);
    assert!(plan.plan_hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn parses_the_sim_adoption_fixture() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../sim/workflows/adoption-report-01.md");
    let plan = WorkflowPlan::parse_file(&fixture).unwrap();
    assert_eq!(plan.name, "adoption-report-01");
    assert_eq!(plan.steps.len(), 3);
    let ids: Vec<&str> = plan.steps.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["collect", "analyze", "deliver"]);
    assert_eq!(plan.steps[0].agent, "repo-scout");
    assert_eq!(plan.steps[1].agent, "pr-analyst");
    assert_eq!(plan.steps[2].agent, "docs-writer");
    assert_eq!(plan.steps[0].model.as_deref(), Some("glm-5.3-flash"));
    assert_eq!(plan.steps[1].needs, ["collect"]);
    assert_eq!(plan.steps[2].needs, ["analyze"]);
    assert_eq!(plan.steps[0].dependants, ["analyze"]);
    assert_eq!(plan.steps[1].dependants, ["deliver"]);
    assert_eq!(plan.steps[2].dependants.len(), 0);
    assert_eq!(plan.steps[0].max_turns, 12);
    assert_eq!(plan.steps[0].retry, 1);
    assert_eq!(plan.steps[0].outputs, ["pr-inventory"]);
    assert_eq!(plan.limits.max_steps, 6);
    assert_eq!(plan.limits.max_total_turns, 60);
    assert_eq!(plan.limits.max_total_tokens, 4_000_000);
    assert_eq!(plan.limits.max_retries_per_step, 1);
    assert!(plan.shared_body.contains("Rules (absolute)"));
    assert!(plan.steps[0].body.contains("{{arg.window}}"));
    assert_eq!(plan.args.len(), 2);
    let resolved = craft_workflow::plan::resolve_args(&plan, &no_args()).unwrap();
    match resolved.get("repos") {
        Some(ArgValue::Seq(items)) => {
            assert_eq!(items, &["photocraft".to_string(), "filmcraft".to_string()]);
        }
        other => panic!("repos default wrong: {other:?}"),
    }
    assert_eq!(plan.plan_hash.len(), 64);
}

#[test]
fn missing_name_is_typed() {
    let text = "---\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\n";
    assert!(matches!(err(text), WorkflowError::MissingName { .. }));
}

#[test]
fn missing_steps_is_typed() {
    let text = "---\nname: t\n---\n";
    assert!(matches!(err(text), WorkflowError::MissingSteps { .. }));
}

#[test]
fn step_count_cap_is_typed() {
    let mut steps = String::new();
    for i in 0..17 {
        steps.push_str(&format!(
            "  - id: s{i}\n    agent: w\n    body: |\n      x\n"
        ));
    }
    let text = format!("---\nname: t\nsteps:\n{steps}---\n");
    assert!(matches!(
        err(&text),
        WorkflowError::StepCountCap {
            count: 17,
            cap: 16,
            ..
        }
    ));
}

#[test]
fn duplicate_step_id_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n  - id: a\n    agent: w\n    body: |\n      y\n---\n";
    assert!(matches!(err(text), WorkflowError::DuplicateStepId { id, .. } if id == "a"));
}

#[test]
fn unknown_need_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    needs: [ghost]\n    body: |\n      x\n---\n";
    assert!(
        matches!(err(text), WorkflowError::UnknownNeed { step, need, .. } if step == "a" && need == "ghost")
    );
}

#[test]
fn duplicate_need_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n  - id: b\n    agent: w\n    needs: [a, a]\n    body: |\n      y\n---\n";
    assert!(
        matches!(err(text), WorkflowError::DuplicateNeed { step, need, .. } if step == "b" && need == "a")
    );
}

#[test]
fn cycle_names_its_path() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    needs: [c]\n    body: |\n      x\n  - id: b\n    agent: w\n    needs: [a]\n    body: |\n      y\n  - id: c\n    agent: w\n    needs: [b]\n    body: |\n      z\n---\n";
    match err(text) {
        WorkflowError::Cycle { path, .. } => assert_eq!(path, "a -> c -> b -> a"),
        other => panic!("expected a cycle, got {other:?}"),
    }
}

#[test]
fn self_need_is_a_cycle() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    needs: [a]\n    body: |\n      x\n---\n";
    assert!(matches!(err(text), WorkflowError::Cycle { .. }));
}

#[test]
fn zero_and_negative_and_huge_limits_are_typed() {
    for bad in [
        "max-steps: 0",
        "max-total-turns: -4",
        "max-total-tokens: 999999999",
        "max-concurrent-steps: x",
    ] {
        let text = format!(
            "---\nname: t\nlimits:\n  {bad}\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\n"
        );
        assert!(
            matches!(err(&text), WorkflowError::BadLimit { .. }),
            "accepted {bad}"
        );
    }
}

#[test]
fn unknown_limit_and_frontmatter_and_step_fields_are_typed() {
    let base = |extra: &str| {
        format!("---\nname: t\n{extra}steps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\n")
    };
    assert!(matches!(
        err(&base("limits:\n  max-oomph: 3\n")),
        WorkflowError::UnknownField { .. }
    ));
    assert!(matches!(
        err(&base("mood: bright\n")),
        WorkflowError::UnknownField { .. }
    ));
    assert!(matches!(
        err(
            "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    tone: snarky\n    body: |\n      x\n---\n"
        ),
        WorkflowError::UnknownField { .. }
    ));
}

#[test]
fn empty_agent_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: \"\"\n    body: |\n      x\n---\n";
    assert!(matches!(err(text), WorkflowError::EmptyAgent { step, .. } if step == "a"));
}

#[test]
fn bad_retry_is_typed() {
    for bad in ["retry: -1", "retry: 9", "retry: lots"] {
        let text = format!(
            "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    retry: {bad}\n    body: |\n      x\n---\n"
        );
        assert!(
            matches!(err(&text), WorkflowError::BadRetry { step, .. } if step == "a"),
            "accepted {bad}"
        );
    }
}

#[test]
fn bad_on_failure_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    on-failure: halt\n    body: |\n      x\n---\n";
    assert!(
        matches!(err(text), WorkflowError::BadOnFailure { step, value, .. } if step == "a" && value == "halt")
    );
}

#[test]
fn zero_max_turns_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    max-turns: 0\n    body: |\n      x\n---\n";
    assert!(matches!(err(text), WorkflowError::BadMaxTurns { step, .. } if step == "a"));
}

#[test]
fn bad_and_missing_ids_are_typed() {
    let bad = "---\nname: t\nsteps:\n  - id: \"a b\"\n    agent: w\n    body: |\n      x\n---\n";
    assert!(matches!(err(bad), WorkflowError::BadId { .. }));
    let missing = "---\nname: t\nsteps:\n  - agent: w\n    body: |\n      x\n---\n";
    assert!(matches!(
        err(missing),
        WorkflowError::MissingStepId { index: 0, .. }
    ));
}

#[test]
fn missing_body_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n---\n";
    assert!(matches!(err(text), WorkflowError::MissingBody { step, .. } if step == "a"));
}

#[test]
fn skip_with_dependants_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    on-failure: skip\n    body: |\n      x\n  - id: b\n    agent: w\n    needs: [a]\n    body: |\n      y\n---\n";
    assert!(
        matches!(err(text), WorkflowError::SkipHasDependants { step, dependent, .. } if step == "a" && dependent == "b")
    );
}

#[test]
fn unknown_arg_ref_is_typed() {
    let text = "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    body: |\n      use {{arg.window}}\n---\n";
    assert!(
        matches!(err(text), WorkflowError::UnknownArgRef { step, name, .. } if step == "a" && name == "window")
    );
}

#[test]
fn prelude_arg_refs_must_be_declared_too() {
    let text = "---\nname: t\nargs:\n  window: { type: string, default: 24h }\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\nuse {{arg.repos}}\n";
    assert!(
        matches!(err(text), WorkflowError::UnknownArgRef { step, name, .. } if step == "prelude" && name == "repos")
    );
}

#[test]
fn bad_arg_specs_are_typed() {
    let unknown_type = "---\nname: t\nargs:\n  w: { type: number }\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\n";
    assert!(matches!(
        err(unknown_type),
        WorkflowError::BadArgSpec { .. }
    ));
    let wrong_default = "---\nname: t\nargs:\n  w: { type: string, default: [a, b] }\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\n";
    assert!(matches!(
        err(wrong_default),
        WorkflowError::BadArgSpec { .. }
    ));
    let unknown_field = "---\nname: t\nargs:\n  w: { type: string, units: hours }\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\n";
    assert!(matches!(
        err(unknown_field),
        WorkflowError::UnknownField { .. }
    ));
}

#[test]
fn yaml_shape_failures_are_parse_errors() {
    for text in [
        "no frontmatter at all",
        "---\nname: t\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x",
        "---\nname: t\nsteps:\n\t- id: a\n---\n",
        "---\nname: t\nname: again\n---\n",
        "---\nname: t\nsteps: 4\n---\n",
    ] {
        assert!(
            matches!(err(text), WorkflowError::Parse { .. }),
            "accepted {text:?}"
        );
    }
}
