//! Step input assembly: prelude + body + prior-step previews, `{{arg.*}}`
//! substitution, and the per-step input hash.

use std::collections::BTreeMap;

use crate::hash::sha256_hex;
use crate::plan::{ArgValue, PREVIEW_CHARS, StepSpec, WorkflowPlan};
use crate::truncate_chars;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInputs {
    pub id: String,
    pub agent: String,
    pub model: Option<String>,
    pub max_turns: u32,
    pub prompt: String,
}

impl StepInputs {
    /// The step's identity in the journal: the plan and args are covered by
    /// the plan hash; the prompt covers the rendered upstream state.
    pub fn input_hash(&self) -> String {
        sha256_hex(&[self.id.as_bytes(), self.prompt.as_bytes()])
    }
}

/// Builds the step's prompt from settled dependency previews only — no step
/// reads another's output mid-wave.
pub fn assemble_step(
    plan: &WorkflowPlan,
    args: &BTreeMap<String, ArgValue>,
    step: &StepSpec,
    dep_previews: &BTreeMap<String, String>,
) -> StepInputs {
    let mut prompt = String::new();
    if !plan.shared_body.is_empty() {
        prompt.push_str(&plan.shared_body);
        prompt.push_str("\n\n");
    }
    prompt.push_str(&substitute(&step.body, args));
    if !step.needs.is_empty() {
        prompt.push_str("\n\n## Prior steps\n");
        for need in &step.needs {
            prompt.push_str(&format!("\n### {need}\n\n"));
            let preview = dep_previews
                .get(need)
                .map(|text| truncate_chars(text, PREVIEW_CHARS))
                .unwrap_or_default();
            prompt.push_str(&preview);
            prompt.push('\n');
        }
    }
    StepInputs {
        id: step.id.clone(),
        agent: step.agent.clone(),
        model: step.model.clone(),
        max_turns: step.max_turns,
        prompt,
    }
}

/// Collects the bounded previews of a step's settled dependencies.
pub fn previews_for(
    step: &StepSpec,
    lookup: impl Fn(&str) -> Option<String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for need in &step.needs {
        if let Some(preview) = lookup(need) {
            out.insert(need.clone(), preview);
        }
    }
    out
}

fn substitute(text: &str, args: &BTreeMap<String, ArgValue>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{arg.") {
        let after = &rest[start + 6..];
        if let Some(end) = after.find("}}") {
            let name = &after[..end];
            out.push_str(&rest[..start]);
            match args.get(name) {
                Some(value) => out.push_str(&render(value)),
                None => out.push_str(&rest[start..start + 6 + end + 2]),
            }
            rest = &after[end + 2..];
        } else {
            out.push_str(&rest[..start + 6]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn render(value: &ArgValue) -> String {
    match value {
        ArgValue::Str(text) => text.clone(),
        ArgValue::Seq(items) => items.join(", "),
    }
}
