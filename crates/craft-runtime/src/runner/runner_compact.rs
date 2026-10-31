//! The runner's auto-compaction state: the attempt count and the last
//! reported input, plus the trigger test that runs before each model
//! call.

use craft_core::provider::Provider;

use crate::compact;
use crate::events::RuntimeEvent;
use crate::runner::RunConfig;
use crate::session::Session;

/// Per-run compaction state: attempts spent and the last input-token
/// report the provider gave.
pub(crate) struct Compaction {
    attempts: u32,
    last_input: Option<u64>,
}

impl Compaction {
    pub(crate) fn new() -> Self {
        Self {
            attempts: 0,
            last_input: None,
        }
    }

    /// Records a folded turn's input report for the next trigger test.
    pub(crate) fn note_turn(&mut self, input_tokens: u64, incomplete: bool) {
        if !incomplete && input_tokens > 0 {
            self.last_input = Some(input_tokens);
        }
    }

    /// Runs the trigger test before a model call and, past the trigger,
    /// makes one summary call and swaps the history prefix for it.
    /// Fail-soft: a failed call counts one attempt and the run
    /// continues; past the attempt bound compaction stops for the rest
    /// of the run.
    pub(crate) async fn maybe_compact<P: Provider + Send + Sync>(
        &mut self,
        provider: &P,
        config: &RunConfig,
        session: &mut Session,
        turn: u32,
        sink: &mut (dyn FnMut(RuntimeEvent) + Send),
    ) {
        let Some(max_context) = config.max_context_tokens else {
            return;
        };
        if self.attempts >= compact::COMPACTION_MAX_ATTEMPTS {
            return;
        }
        let used = compact::estimate_used(self.last_input, session.messages());
        if !compact::should_compact(used, max_context, config.max_input_tokens) {
            return;
        }
        match compact::compact_once(provider, config, session).await {
            Ok((kept, summary_bytes)) => {
                self.attempts += 1;
                self.last_input = None;
                sink(RuntimeEvent::Compacted {
                    turn,
                    attempt: self.attempts,
                    kept_messages: kept,
                    summary_bytes,
                });
            }
            Err(_) => {
                self.attempts += 1;
            }
        }
    }
}
