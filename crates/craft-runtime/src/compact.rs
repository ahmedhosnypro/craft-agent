//! Auto-compaction: when a run sets a context cap, the loop estimates
//! the used context before each model call and, past the trigger, asks
//! the model for one summary and swaps the history prefix for it,
//! keeping the last few messages verbatim. Fail-soft: a failed
//! compaction call counts one attempt and the run continues; past the
//! attempt bound compaction stops for the rest of the run.

use std::sync::Arc;

use craft_core::error::ProviderError;
use craft_core::events::{FoldError, TurnFold};
use craft_core::provider::Provider;
use craft_core::types::{ChatMessage, ModelRequest};
use futures::StreamExt;

use crate::budget::fold_error;
use crate::runner::RunConfig;
use crate::session::Session;

/// Trigger fraction numerator over denominator (0.85), integer math.
pub(crate) const COMPACTION_TRIGGER_NUM: u64 = 85;
pub(crate) const COMPACTION_TRIGGER_DEN: u64 = 100;
/// Reserved headroom against the context cap.
pub(crate) const COMPACTION_RESERVED: u64 = 50_000;
/// Compaction calls per run before compaction stops.
pub(crate) const COMPACTION_MAX_ATTEMPTS: u32 = 3;
/// Trailing messages kept verbatim after a compaction.
pub(crate) const COMPACTION_KEEP_TAIL: usize = 4;
/// Output cap for the summary call.
pub(crate) const COMPACTION_SUMMARY_MAX_TOKENS: u32 = 131_072;

/// The instruction that rides the compaction call.
pub(crate) const COMPACTION_INSTRUCTION: &str = "Summarize the conversation so far into a compact brief: goals, decisions, file paths, open questions, and the next step. The summary replaces the earlier history.";

/// Marker prefix on the summary message in the compacted history.
const COMPACTION_SUMMARY_PREFIX: &str = "[compacted summary] ";

/// The effective input-side limit: the explicit input cap or the
/// context cap.
pub(crate) fn effective_max_input(max_context: u64, max_input: Option<u64>) -> u64 {
    max_input.unwrap_or(max_context)
}

/// The trigger test in saturating integer arithmetic: used at or past
/// 85% of the effective input limit, or used plus the reserve at or
/// past the context cap.
pub(crate) fn should_compact(used: u64, max_context: u64, max_input: Option<u64>) -> bool {
    let input_effective = effective_max_input(max_context, max_input);
    let ratio = used.saturating_mul(COMPACTION_TRIGGER_DEN)
        >= input_effective.saturating_mul(COMPACTION_TRIGGER_NUM);
    let reserve = used.saturating_add(COMPACTION_RESERVED) >= max_context;
    ratio || reserve
}

/// The used-context estimate: the provider's last-turn input report
/// wins; otherwise a chars/4 walk over the session messages.
pub(crate) fn estimate_used(last_input: Option<u64>, messages: &[Arc<ChatMessage>]) -> u64 {
    if let Some(input) = last_input.filter(|value| *value > 0) {
        return input;
    }
    let mut chars = 0u64;
    for message in messages {
        chars = chars.saturating_add(message_chars(message));
    }
    chars.saturating_add(3) / 4
}

/// One message's approximate char count: text plus tool payloads.
fn message_chars(message: &ChatMessage) -> u64 {
    let bytes = match message {
        ChatMessage::User { text, .. } => text.len(),
        ChatMessage::Assistant {
            text, tool_calls, ..
        } => {
            text.len()
                + tool_calls
                    .iter()
                    .map(|call| call.id.len() + call.name.len() + call.arguments.len())
                    .sum::<usize>()
        }
        ChatMessage::Tool {
            call_id, content, ..
        } => content.len() + call_id.len(),
    };
    bytes as u64
}

/// The compacted history: one summary message plus the last `keep`
/// entries verbatim.
pub(crate) fn compacted_history(
    messages: &[Arc<ChatMessage>],
    summary: &str,
    keep: usize,
) -> Vec<Arc<ChatMessage>> {
    let keep = keep.min(messages.len());
    let mut compacted = Vec::with_capacity(keep + 1);
    compacted.push(Arc::new(ChatMessage::User {
        text: format!("{COMPACTION_SUMMARY_PREFIX}{summary}"),
        images: Vec::new(),
    }));
    compacted.extend_from_slice(&messages[messages.len() - keep..]);
    compacted
}

/// One compaction call: stream a summary turn over the full history,
/// fold it, and swap the session's in-memory prefix. Returns the kept
/// tail length and the summary's byte length. Any failure is a typed
/// error, never a panic.
pub(crate) async fn compact_once<P: Provider + Send + Sync>(
    provider: &P,
    config: &RunConfig,
    session: &mut Session,
) -> Result<(usize, usize), ProviderError> {
    let mut messages = session.messages().to_vec();
    messages.push(Arc::new(ChatMessage::User {
        text: String::from(COMPACTION_INSTRUCTION),
        images: Vec::new(),
    }));
    let request = ModelRequest {
        system: config.system.clone(),
        messages,
        tools: Vec::new(),
        max_tokens: config.max_tokens.min(COMPACTION_SUMMARY_MAX_TOKENS),
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    };
    let mut stream = provider.stream(request).await?;
    let mut fold = TurnFold::new(config.turn_caps);
    while let Some(item) = stream.next().await {
        fold.push(item?).map_err(fold_error)?;
    }
    let folded = fold.finish().map_err(|err| match err {
        FoldError::Unfinished => ProviderError::StreamClosed,
        err => fold_error(err),
    })?;
    if folded.text.is_empty() {
        return Err(ProviderError::EmptyResponse);
    }
    let history = session.messages().to_vec();
    let kept = COMPACTION_KEEP_TAIL.min(history.len());
    let summary_bytes = folded.text.len();
    session.replace_messages(compacted_history(
        &history,
        &folded.text,
        COMPACTION_KEEP_TAIL,
    ));
    Ok((kept, summary_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Arc<ChatMessage> {
        Arc::new(ChatMessage::User {
            text: String::from(text),
            images: Vec::new(),
        })
    }

    #[test]
    fn trigger_fires_at_eighty_five_percent_of_the_effective_input() {
        // max_input unset: the context cap is the effective input limit.
        assert!(!should_compact(849_999, 1_000_000, None));
        assert!(should_compact(850_000, 1_000_000, None));
        // an explicit input cap narrows the trigger below the context cap.
        assert!(!should_compact(424_999, 1_000_000, Some(500_000)));
        assert!(should_compact(425_000, 1_000_000, Some(500_000)));
        // an explicit input cap above the context cap still triggers on
        // the reserve rule.
        assert!(should_compact(960_000, 1_000_000, Some(2_000_000)));
    }

    #[test]
    fn trigger_fires_on_the_reserve_rule_near_the_context_cap() {
        // An input cap above the context cap isolates the reserve arm:
        // the ratio arm would need 1.7M used.
        assert!(!should_compact(949_999, 1_000_000, Some(2_000_000)));
        assert!(should_compact(950_000, 1_000_000, Some(2_000_000)));
        assert!(should_compact(u64::MAX, 1_000_000, None), "saturating math");
    }

    #[test]
    fn estimate_prefers_the_provider_report_and_falls_back_to_chars_over_four() {
        let messages = vec![user(&"x".repeat(400))];
        assert_eq!(estimate_used(Some(12_345), &messages), 12_345);
        assert_eq!(
            estimate_used(Some(0), &messages),
            100,
            "zero report falls back"
        );
        assert_eq!(estimate_used(None, &messages), 100);
        assert_eq!(estimate_used(None, &[]), 0);
    }

    #[test]
    fn compacted_history_keeps_the_last_four_verbatim_behind_one_summary() {
        let messages: Vec<Arc<ChatMessage>> =
            (0..10).map(|index| user(&format!("m{index}"))).collect();
        let compacted = compacted_history(&messages, "brief", COMPACTION_KEEP_TAIL);
        assert_eq!(compacted.len(), 5);
        let ChatMessage::User { text, .. } = compacted[0].as_ref() else {
            panic!("the head is the summary user message");
        };
        assert_eq!(text, "[compacted summary] brief");
        for (slot, index) in (6..10).enumerate() {
            let ChatMessage::User { text, .. } = compacted[slot + 1].as_ref() else {
                panic!("tail entries stay verbatim");
            };
            assert_eq!(text, &format!("m{index}"));
        }
        // A short history keeps everything after the summary.
        let short = vec![user("a"), user("b")];
        assert_eq!(
            compacted_history(&short, "s", COMPACTION_KEEP_TAIL).len(),
            3
        );
    }
}
