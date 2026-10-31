//! The runtime step ladder: replay policy and backoff for a model step
//! whose stream died mid-turn, plus the `CRAFT_AGENT_STEP_RETRIES` knob.
//!
//! Retries live at exactly one level per failure. The provider seam
//! splits the levels structurally: an error surfacing from `stream()`
//! is connect-phase and already climbed the adapter's connect ladder,
//! so the runtime never replays it; an error surfacing inside the event
//! stream was never retried anywhere and is the step ladder's to replay.

use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use craft_core::error::{ProviderError, rate_limit_wait};
use craft_core::types::StopReason;

/// The env knob bounding total attempts per model step.
pub const STEP_RETRIES_ENV: &str = "CRAFT_AGENT_STEP_RETRIES";

/// Attempts per step when the knob is unset or empty.
pub const DEFAULT_STEP_ATTEMPTS: u32 = 3;

const BACKOFF_INITIAL_MS: u64 = 500;
const BACKOFF_CAP: Duration = Duration::from_secs(32);

/// One env value into the attempt bound: unset or empty keeps the
/// default, zero restores fail-fast (a single attempt), and anything
/// unparseable is a typed error naming the knob.
pub fn step_attempts(value: Option<&str>) -> Result<u32, ProviderError> {
    let Some(raw) = value.filter(|raw| !raw.is_empty()) else {
        return Ok(DEFAULT_STEP_ATTEMPTS);
    };
    let attempts = raw.trim().parse::<u32>().map_err(|_| invalid(raw))?;
    Ok(attempts.max(1))
}

/// The attempt bound from the live process environment.
pub fn step_attempts_from_env() -> Result<u32, ProviderError> {
    match std::env::var(STEP_RETRIES_ENV) {
        Ok(value) => step_attempts(Some(&value)),
        Err(std::env::VarError::NotPresent) => step_attempts(None),
        Err(err) => Err(invalid(&err.to_string())),
    }
}

/// The patience window from the live environment: how long a step may
/// keep replaying transient failures after its attempt bound.
pub fn step_patience_from_env() -> Result<Duration, ProviderError> {
    craft_core::error::retry_patience_from_env()
        .map_err(|detail| ProviderError::InvalidRequest { detail })
}

fn invalid(raw: &str) -> ProviderError {
    ProviderError::InvalidRequest {
        detail: format!(
            "{STEP_RETRIES_ENV} must be a whole number of attempts, 0 to fail fast; got `{raw}`"
        ),
    }
}

/// The replay decision for one failure: a stable `turn_retried` reason
/// token and the wait before the next attempt, or None when the run
/// fails with the error itself.
pub(crate) type Replay = Option<(&'static str, Duration)>;

/// Step-retryability of a mid-stream death, encoded once for every
/// taxonomy row: the stream classes replay, a rate limit replays at most
/// once with a bounded wait, and the deterministic classes fail the run.
pub(crate) fn stream_replay(err: &ProviderError, attempt: u32) -> Replay {
    let backoff = jittered(ladder(attempt));
    let replay = match err {
        ProviderError::Transport { .. } => ("transport", backoff),
        ProviderError::Transient { .. } => ("transient", backoff),
        ProviderError::Timeout { .. } => ("timeout", backoff),
        ProviderError::IdleTimeout => ("idle_timeout", backoff),
        ProviderError::StreamClosed => ("stream_closed", backoff),
        ProviderError::RateLimit { retry_after } => {
            // An upstream 429 replays at most once: a step retry must
            // not re-dispatch a whole fan-out into an exhausted quota.
            if attempt > 1 {
                return None;
            }
            (
                "rate_limit",
                rate_limit_wait(*retry_after, backoff, BACKOFF_CAP),
            )
        }
        ProviderError::Auth { .. }
        | ProviderError::InvalidRequest { .. }
        | ProviderError::ContextWindow { .. }
        | ProviderError::PayloadTooLarge { .. }
        | ProviderError::QuotaExhausted { .. }
        | ProviderError::EmptyResponse => return None,
    };
    Some(replay)
}

/// An empty-but-complete turn replays on the same ladder unless the
/// provider filtered it — a filtered response is deterministic.
pub(crate) fn empty_replay(stop_reason: &StopReason, attempt: u32) -> Replay {
    if matches!(stop_reason, StopReason::ContentFilter) {
        return None;
    }
    Some(("empty_response", jittered(ladder(attempt))))
}

fn ladder(attempt: u32) -> Duration {
    let step = attempt.saturating_sub(1);
    let grown = BACKOFF_INITIAL_MS.checked_shl(step).unwrap_or(u64::MAX);
    Duration::from_millis(grown.min(BACKOFF_CAP.as_millis() as u64))
}

/// ±25% around a ladder step, seeded from the wall clock; the ladder
/// itself bounds the spread's input, so the arithmetic cannot overflow.
fn jittered(base: Duration) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |now| now.as_nanos() as u64);
    let mut hasher = DefaultHasher::new();
    hasher.write_u64(nanos);
    hasher.write_u64(base.as_nanos() as u64);
    let percent = 75 + hasher.finish() % 51;
    let millis = (base.as_millis() * u128::from(percent)) / 100;
    Duration::from_millis(millis as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn within(actual: Duration, base_ms: u64) -> bool {
        let low = Duration::from_millis(base_ms * 75 / 100);
        let high = Duration::from_millis(base_ms * 125 / 100);
        low <= actual && actual <= high
    }

    #[test]
    fn every_stream_class_replays_with_jittered_ladder_steps() {
        for (index, err) in [
            ProviderError::Transport {
                detail: String::from("wire reset"),
            },
            ProviderError::Transient {
                detail: String::from("502"),
            },
            ProviderError::Timeout {
                detail: String::from("deadline"),
            },
            ProviderError::IdleTimeout,
            ProviderError::StreamClosed,
        ]
        .into_iter()
        .enumerate()
        {
            for attempt in 1..=9u32 {
                let Some((_, wait)) = stream_replay(&err, attempt) else {
                    panic!("row {index} attempt {attempt} must replay");
                };
                let base_ms = 500u64
                    .checked_shl(attempt.saturating_sub(1))
                    .unwrap_or(u64::MAX)
                    .min(BACKOFF_CAP.as_millis() as u64);
                assert!(
                    within(wait, base_ms),
                    "row {index} attempt {attempt}: {wait:?}"
                );
            }
        }
    }

    #[test]
    fn deterministic_rows_never_replay() {
        for err in [
            ProviderError::Auth {
                detail: String::from("bad key"),
            },
            ProviderError::InvalidRequest {
                detail: String::from("bad shape"),
            },
            ProviderError::ContextWindow {
                detail: String::from("too long"),
            },
            ProviderError::PayloadTooLarge {
                detail: String::from("too big"),
            },
            ProviderError::QuotaExhausted {
                detail: String::from("spent"),
            },
            ProviderError::EmptyResponse,
        ] {
            assert!(
                stream_replay(&err, 1).is_none(),
                "{err:?} is deterministic and must fail the run"
            );
        }
    }

    #[test]
    fn rate_limit_honors_retry_after_clamped_to_the_cap() {
        let (_, wait) = stream_replay(
            &ProviderError::RateLimit {
                retry_after: Some(Duration::from_secs(10)),
            },
            1,
        )
        .expect("the first rate limit replays");
        assert_eq!(wait, Duration::from_secs(10));
        let (_, wait) = stream_replay(
            &ProviderError::RateLimit {
                retry_after: Some(Duration::from_secs(9999)),
            },
            1,
        )
        .expect("the first rate limit replays");
        assert_eq!(wait, BACKOFF_CAP);
    }

    #[test]
    fn rate_limit_replays_at_most_once_per_step() {
        for retry_after in [None, Some(Duration::ZERO), Some(Duration::from_secs(5))] {
            let err = ProviderError::RateLimit { retry_after };
            assert!(
                stream_replay(&err, 1).is_some(),
                "the first 429 replays once: {err:?}"
            );
            for attempt in 2..=9u32 {
                assert!(
                    stream_replay(&err, attempt).is_none(),
                    "attempt {attempt} must fail the run instead of re-dispatching: {err:?}"
                );
            }
        }
    }

    #[test]
    fn zero_or_absent_retry_after_never_waits_zero() {
        for retry_after in [None, Some(Duration::ZERO)] {
            let err = ProviderError::RateLimit { retry_after };
            let Some((reason, wait)) = stream_replay(&err, 1) else {
                panic!("the first rate limit replays: {err:?}");
            };
            assert_eq!(reason, "rate_limit");
            assert!(
                wait > Duration::ZERO && within(wait, 500),
                "a zero/absent header rides the jittered ladder, not an immediate retry: {wait:?}"
            );
        }
    }

    #[test]
    fn empty_turns_replay_unless_the_provider_filtered() {
        assert!(empty_replay(&StopReason::EndTurn, 1).is_some());
        assert!(empty_replay(&StopReason::ToolUse, 1).is_some());
        assert!(empty_replay(&StopReason::ContentFilter, 1).is_none());
        let Some((reason, wait)) = empty_replay(&StopReason::EndTurn, 1) else {
            panic!("plain empties replay");
        };
        assert_eq!(reason, "empty_response");
        assert!(within(wait, 500));
    }
}
