//! The `poll_agents` tool: collects the envelopes of detached children
//! parked in the run's subagent hub, optionally waiting bounded for the
//! next one to settle. A poll collecting nothing is normal state —
//! work is still outstanding — never a tool error. A launched child
//! waiting in a poll releases its pool slot for the duration: the
//! waiter is not working, and holding the slot would deadlock the very
//! stage children it waits for behind siblings that are also waiting.

use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;

use craft_tools::ToolError;
use craft_tools::event::ToolStream;
use craft_tools::schema::{Prop, Schema};
use craft_tools::tool::{Tool, ToolCtx};
use craft_tools::{ToolEvent, ToolResult};

use crate::subagent::SubagentWiring;

const DESCRIPTION: &str = "Collects the envelopes of detached \
subagents launched with `detached: true`. Pass ids to collect specific \
children, or omit the list to drain every report dispatched at your own \
level — a pipeline's stage children stay with the pipeline that spawned \
them. With `waitSecs` the poll waits up to that long for the next child \
to settle before answering; 0 returns immediately. Ids that are unknown \
or were already collected are a typed error.";

const PROPS: &[Prop] = &[
    Prop {
        name: "ids",
        ty: Schema::StrList,
        required: false,
        desc: "Detached child ids to collect; omitted drains every settled report.",
    },
    Prop {
        name: "waitSecs",
        ty: Schema::Int,
        required: false,
        desc: "Wait up to this many seconds for a settle, 0 to 3600; default 0.",
    },
];

/// Ids accepted per poll.
const IDS_CAP: usize = 128;
/// The longest bounded wait one poll may hold.
const WAIT_CEILING: u64 = 3_600;
/// The sleep between wait ticks, so a wait never spins.
const TICK: Duration = Duration::from_millis(200);

pub(crate) struct PollAgentsTool {
    wiring: SubagentWiring,
}

impl PollAgentsTool {
    pub(crate) fn new(wiring: SubagentWiring) -> Self {
        Self { wiring }
    }
}

impl Tool for PollAgentsTool {
    fn name(&self) -> &'static str {
        "poll_agents"
    }

    fn description(&self) -> &'static str {
        DESCRIPTION
    }

    fn params_schema(&self) -> Schema {
        Schema::object(
            "Collects detached subagent envelopes by id or in bulk.",
            PROPS,
        )
    }

    fn execute(&self, args: Value, _ctx: &ToolCtx) -> ToolStream {
        let ids = match requested_ids(&args) {
            Ok(ids) => ids,
            Err(msg) => return invalid(&args, msg),
        };
        let wait = match wait_secs(&args) {
            Ok(wait) => wait,
            Err(msg) => return invalid(&args, msg),
        };
        let hub = self.wiring.hub.clone();
        let unknown: Vec<String> = ids.iter().filter(|id| !hub.knows(id)).cloned().collect();
        if !unknown.is_empty() {
            return invalid(
                &args,
                format!(
                    "unknown agent ids (already collected, or from an earlier process): {}",
                    unknown.join(", ")
                ),
            );
        }
        let depth = self.wiring.depth;
        futures::stream::once(async move { Ok(collect(&hub, &ids, wait, depth).await) }).boxed()
    }
}

/// Waits bounded for the requested work to settle, then renders
/// everything collected. A poll answers as soon as at least one
/// requested id parks, so several polls may drain one wave; the wait
/// only bounds how long one call holds.
///
/// A launched child (`depth > 0`) yields its slot once it must actually
/// wait: the release opens a pool slot immediately, so the stage
/// children it dispatched queue-free run while it waits, and it
/// re-acquires the count the moment the wait ends. The root parent
/// never yields — it holds no slot.
async fn collect(
    hub: &crate::subagent::SubagentHub,
    ids: &[String],
    wait: u64,
    depth: u8,
) -> ToolEvent {
    if wait == 0 {
        return terminal(hub, hub.take_parked(ids, depth));
    }
    let since = std::time::Instant::now();
    let mut yielded = false;
    loop {
        let collected = hub.take_parked(ids, depth);
        if !collected.is_empty() {
            if yielded {
                hub.end_wait();
            }
            return terminal(hub, collected);
        }
        if since.elapsed().as_secs() >= wait {
            if yielded {
                hub.end_wait();
            }
            return terminal(hub, Vec::new());
        }
        if !yielded && depth > 0 {
            hub.begin_wait();
            yielded = true;
        }
        tokio::time::sleep(TICK).await;
    }
}

/// The poll's single terminal result.
fn terminal(
    hub: &crate::subagent::SubagentHub,
    collected: Vec<(String, crate::subagent::ParkedChild)>,
) -> ToolEvent {
    let mut handoffs = Vec::with_capacity(collected.len());
    for (id, parked) in collected {
        handoffs.push(
            parked
                .report
                .into_handoff(&id, &parked.profile, parked.budget_remaining),
        );
    }
    let outstanding = hub.outstanding();
    let mut output = if handoffs.is_empty() {
        String::from("poll: 0 collected\n")
    } else {
        let mut rendered = craft_core::subagent::handoff::render_batch(&handoffs);
        rendered.push('\n');
        rendered
    };
    if outstanding.is_empty() {
        output.push_str("outstanding: none\n");
    } else {
        output.push_str("outstanding: ");
        output.push_str(&outstanding.join(", "));
        output.push('\n');
    }
    ToolEvent::Terminal(ToolResult { output })
}

/// A typed invalid-argument error naming the tool.
fn invalid(_args: &Value, msg: String) -> ToolStream {
    craft_tools::event::error_stream(ToolError::InvalidArgs {
        tool: String::from("poll_agents"),
        msg,
    })
}

/// The requested ids: absent or null drains everything settled; a list
/// is bounded and must carry non-empty strings.
fn requested_ids(args: &Value) -> Result<Vec<String>, String> {
    let Value::Object(map) = args else {
        return Ok(Vec::new());
    };
    match map.get("ids") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(list)) => {
            if list.is_empty() || list.len() > IDS_CAP {
                return Err(format!(
                    "ids must carry 1..={IDS_CAP} entries, got {}",
                    list.len()
                ));
            }
            let mut ids = Vec::with_capacity(list.len());
            for entry in list {
                match entry {
                    Value::String(text) if !text.trim().is_empty() => ids.push(text.clone()),
                    other => {
                        return Err(format!(
                            "every id must be a non-empty string; got `{other}`"
                        ));
                    }
                }
            }
            Ok(ids)
        }
        Some(other) => Err(format!("ids must be an array of strings; got `{other}`")),
    }
}

/// The bounded wait, in seconds.
fn wait_secs(args: &Value) -> Result<u64, String> {
    let Value::Object(map) = args else {
        return Ok(0);
    };
    match map.get("waitSecs") {
        None | Some(Value::Null) => Ok(0),
        Some(Value::Number(number)) => match number.as_u64() {
            Some(value) if value <= WAIT_CEILING => Ok(value),
            _ => Err(format!("waitSecs must be 0..={WAIT_CEILING}")),
        },
        Some(other) => Err(format!("waitSecs must be a whole number; got `{other}`")),
    }
}
