//! Run-budget accounting: cross-turn usage sums and the fail-closed
//! token-budget check.

use craft_core::error::ProviderError;
use craft_core::events::FoldError;
use craft_core::types::Usage;

pub(crate) fn zero_usage() -> Usage {
    Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

/// Sums per-turn reports into the run total; repeats inside one turn were
/// already merged by the fold. The incomplete flag ORs, fail-closed.
pub(crate) fn sum_usage(running: Usage, turn: Usage) -> Usage {
    Usage {
        input_tokens: running.input_tokens.saturating_add(turn.input_tokens),
        output_tokens: running.output_tokens.saturating_add(turn.output_tokens),
        cache_read_tokens: running
            .cache_read_tokens
            .saturating_add(turn.cache_read_tokens),
        cache_write_tokens: running
            .cache_write_tokens
            .saturating_add(turn.cache_write_tokens),
        reasoning_tokens: running
            .reasoning_tokens
            .saturating_add(turn.reasoning_tokens),
        incomplete: running.incomplete || turn.incomplete,
    }
}

/// None while the run may continue; Some(detail) when the budget fails
/// closed — exceeded, or unverifiable because usage is incomplete.
///
/// The usage buckets are disjoint: the provider parse splits the
/// prompt into input plus cache-read (and cache-write where reported)
/// and the completion into output plus reasoning. Spend sums every
/// bucket, so the cap bounds the full context the provider processed —
/// cached prefixes included — and the sum matches the provider's own
/// per-turn totals without overlap.
pub(crate) fn budget_status(total: &Usage, max: u64) -> Option<String> {
    if total.incomplete {
        return Some(String::from(
            "token budget cannot be verified: the provider reported incomplete usage",
        ));
    }
    let spent = total
        .input_tokens
        .saturating_add(total.cache_read_tokens)
        .saturating_add(total.cache_write_tokens)
        .saturating_add(total.output_tokens)
        .saturating_add(total.reasoning_tokens);
    (spent >= max).then(|| format!("token budget exceeded: {spent} tokens spent, {max} allowed"))
}

pub(crate) fn fold_error(err: FoldError) -> ProviderError {
    ProviderError::InvalidRequest {
        detail: format!("model stream assembly failed: {err}"),
    }
}
