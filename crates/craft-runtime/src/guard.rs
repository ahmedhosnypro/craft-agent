//! Stationarity guard: identical repeated tool calls, compared by a
//! canonicalized name-plus-arguments signature, nudge after 4 and stop
//! after 8.

use craft_core::types::ToolCall;
use serde_json::Value;

const NUDGE_AFTER: usize = 4;
const STOP_AFTER: usize = 8;

/// The user message injected before the next request once a call repeats.
pub(crate) const NUDGE_TEXT: &str =
    "The same tool call was repeated 4 times in a row. Change your approach or finish the task.";

#[derive(Default)]
pub(crate) struct Stationarity {
    signature: Option<String>,
    count: usize,
    nudged: bool,
}

pub(crate) enum Guard {
    Continue,
    Nudge,
    Stop,
}

impl Stationarity {
    /// Observes a call after it executed: a repeated signature counts the
    /// streak up, any different call resets it.
    pub(crate) fn observe(&mut self, call: &ToolCall) -> Guard {
        let signature = call_signature(call);
        if self.signature.as_deref() == Some(signature.as_str()) {
            self.count += 1;
        } else {
            self.signature = Some(signature);
            self.count = 1;
            self.nudged = false;
        }
        if self.count >= STOP_AFTER {
            return Guard::Stop;
        }
        if self.count == NUDGE_AFTER && !self.nudged {
            self.nudged = true;
            return Guard::Nudge;
        }
        Guard::Continue
    }
}

/// Tool name plus arguments with object keys sorted recursively, so
/// equivalent JSON that differs only in key order or spacing counts as
/// the same call; unparseable arguments fall back to their raw text.
pub(crate) fn call_signature(call: &ToolCall) -> String {
    let args = serde_json::from_str::<Value>(&call.arguments)
        .unwrap_or_else(|_| Value::String(call.arguments.clone()));
    format!("{}\u{1}{}", call.name, canonical(&args))
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{key}:{}", canonical(map.get(key).unwrap_or(&Value::Null))))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", parts.join(","))
        }
        Value::String(text) => format!("{text:?}"),
        other => other.to_string(),
    }
}
