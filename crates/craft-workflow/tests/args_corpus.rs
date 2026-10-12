//! Args and file-edge corpus: arg resolution rules and file caps.
#![allow(clippy::unwrap_used)]

mod common;

use craft_workflow::{ArgValue, WorkflowError, WorkflowPlan};

fn ok(text: &str) -> WorkflowPlan {
    WorkflowPlan::parse("t.md", text).unwrap()
}

#[test]
fn resolve_args_rejects_missing_undeclared_and_mismatched() {
    let plan = ok(
        "---\nname: t\nargs:\n  w: { type: string }\n  r: { type: array, default: [a] }\nsteps:\n  - id: s\n    agent: w\n    body: |\n      x\n---\n",
    );
    let mut provided = std::collections::BTreeMap::new();
    provided.insert("extra".to_string(), ArgValue::Str("nope".to_string()));
    assert!(matches!(
        craft_workflow::plan::resolve_args(&plan, &provided).unwrap_err(),
        WorkflowError::ArgNotDeclared { name } if name == "extra"
    ));
    let provided = std::collections::BTreeMap::new();
    match craft_workflow::plan::resolve_args(&plan, &provided) {
        Err(WorkflowError::ArgsMissing { names }) => assert_eq!(names, "w"),
        other => panic!("expected missing args, got {other:?}"),
    }
    let mut provided = std::collections::BTreeMap::new();
    provided.insert("w".to_string(), ArgValue::Seq(vec!["a".to_string()]));
    assert!(matches!(
        craft_workflow::plan::resolve_args(&plan, &provided).unwrap_err(),
        WorkflowError::BadArgValue { name, .. } if name == "w"
    ));
    let mut provided = std::collections::BTreeMap::new();
    provided.insert("w".to_string(), ArgValue::Str("24h".to_string()));
    let resolved = craft_workflow::plan::resolve_args(&plan, &provided).unwrap();
    match resolved.get("r") {
        Some(ArgValue::Seq(items)) => assert_eq!(items, &["a".to_string()]),
        other => panic!("default not filled: {other:?}"),
    }
}

#[test]
fn oversized_and_non_utf8_files_are_parse_errors() {
    let dir = common::temp_dir("plancap");
    let big = dir.join("big.md");
    std::fs::write(&big, [b'x'; 300_000]).unwrap();
    assert!(matches!(
        WorkflowPlan::parse_file(&big).unwrap_err(),
        WorkflowError::Parse { .. }
    ));
    let binary = dir.join("bin.md");
    std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
    assert!(matches!(
        WorkflowPlan::parse_file(&binary).unwrap_err(),
        WorkflowError::Parse { .. }
    ));
}

#[test]
fn comments_inline_and_full_line_are_handled() {
    let text = "---\nname: t # the name\nlimits:\n  max-steps: 8 # cap\nsteps:\n  - id: a\n    agent: w\n    body: |\n      x\n---\n";
    let plan = ok(text);
    assert_eq!(plan.name, "t");
    assert_eq!(plan.limits.max_steps, 8);
}

#[test]
fn flow_style_steps_and_needs_parse() {
    let text = "---\nname: t\nsteps:\n  - { id: a, agent: w, needs: [], body: do it }\n---\n";
    let plan = ok(text);
    assert_eq!(plan.steps[0].id, "a");
    assert_eq!(plan.steps[0].agent, "w");
    assert_eq!(plan.steps[0].body, "do it");
    assert!(plan.steps[0].needs.is_empty());
}
