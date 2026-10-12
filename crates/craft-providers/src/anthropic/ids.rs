//! Tool-call id sanitation: one deterministic wire-safe form for
//! tool_use ids and the tool_result ids that pair with them.

use std::collections::{HashMap, HashSet};

/// Wire ids cap out at 64 bytes.
const MAX_ID_BYTES: usize = 64;

/// Original-to-wire id map for one encoded request: assigns tool_use
/// ids and resolves the tool_result ids that pair with them.
#[derive(Default)]
pub(super) struct IdMap {
    assigned: HashMap<String, String>,
    taken: HashSet<String>,
}

impl IdMap {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// The wire form for a tool_use id; a collision with an earlier
    /// sanitized id gets a numeric suffix, re-truncated to the cap.
    pub(super) fn assign(&mut self, raw: &str) -> String {
        if let Some(hit) = self.assigned.get(raw) {
            return hit.clone();
        }
        let base = sanitize(raw);
        let mut winner = base.clone();
        for suffix in 2..=(self.taken.len() + 2) {
            if !self.taken.contains(&winner) {
                break;
            }
            let tag = format!("-{suffix}");
            let room = MAX_ID_BYTES.saturating_sub(tag.len());
            winner = format!("{}{tag}", truncate(&base, room));
        }
        self.assigned.insert(raw.to_owned(), winner.clone());
        self.taken.insert(winner.clone());
        winner
    }

    /// The wire form for a tool_result id: registered ids keep their
    /// assigned pair, unregistered ids sanitize directly.
    pub(super) fn resolve(&self, raw: &str) -> String {
        self.assigned
            .get(raw)
            .cloned()
            .unwrap_or_else(|| sanitize(raw))
    }
}

/// [A-Za-z0-9_-] kept, every other char becomes '-', capped at 64
/// bytes on a char boundary.
fn sanitize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_ID_BYTES));
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    truncate(&out, MAX_ID_BYTES)
}

fn truncate(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_owned();
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).map_or_else(String::new, str::to_owned)
}
