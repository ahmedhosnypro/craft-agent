//! The run's endgame: the detached pool drains, queued envelope events
//! flush into the sink, and the terminal event with its outcome are
//! emitted — one path so the event stream is complete whatever the exit.

use craft_core::types::Usage;

use crate::detached::{DETACHED_DRAIN, drain_detached};
use crate::events::{CancelCategory, RuntimeEvent};
use crate::outcome::{ExitStatus, RunOutcome};
use crate::runner::RunConfig;

/// Every ending passes through here; the root also drains the detached
/// pool first — children still in flight settle typed (bounded) and
/// their envelope events reach the sink before the terminal event. A
/// child runner shares the hub but owns nothing in it: only the root
/// drains and flushes.
pub(crate) async fn finish(
    config: &RunConfig,
    sink: &mut (dyn FnMut(RuntimeEvent) + Send),
    exit: ExitStatus,
    category: CancelCategory,
    detail: String,
    usage: Usage,
    turns: u32,
) -> RunOutcome {
    if let Some(wiring) = config.subagents.as_ref().filter(|wiring| wiring.is_root()) {
        drain_detached(wiring, DETACHED_DRAIN).await;
        for event in wiring.hub.take_events(turns) {
            sink(event);
        }
    }
    let detail = config.redactor.redact(&detail).into_owned();
    sink(RuntimeEvent::RunFinished {
        exit_code: exit.code(),
        category,
        detail: detail.clone(),
        usage,
        turns,
    });
    RunOutcome {
        exit,
        category,
        detail,
        usage,
        turns,
        session_path: config.session_path.clone(),
    }
}
