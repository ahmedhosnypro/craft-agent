//! Next-request assembly: the registry's cached tool specs plus the
//! session transcript, with tool results older than the result window
//! riding as size stubs, oversized in-window results as head plus a
//! spill pointer, and assistant messages kept verbatim so replayed
//! native blocks ride their turns unchanged.

use craft_core::types::{ModelRequest, ToolSpec};
use craft_tools::Registry;

use crate::prepare::Prepared;
use crate::runner::RunConfig;
use crate::trim;
use crate::truncate;

/// The request for the next model step: the system prompt, the trimmed
/// and capped transcript from the prepared run state, the cached tool
/// specs, and the per-request output cap. Async because capping an
/// oversized result may spill on a writer thread.
pub(crate) async fn build(
    config: &RunConfig,
    registry: &Registry,
    prepared: &mut Prepared,
) -> ModelRequest {
    let tools = registry
        .names()
        .iter()
        .filter_map(|name| registry.spec(name))
        .map(|spec| ToolSpec {
            name: spec.name.clone(),
            description: spec.description.clone(),
            parameters: spec.schema.clone(),
        })
        .collect();
    let window = prepared.result_window;
    let messages = truncate::cap(
        trim::assemble(prepared.session.messages(), window),
        &mut prepared.truncate,
    )
    .await;
    ModelRequest {
        system: config.system.clone(),
        messages,
        tools,
        max_tokens: config.max_tokens,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: config.thinking_level.clone(),
    }
}
