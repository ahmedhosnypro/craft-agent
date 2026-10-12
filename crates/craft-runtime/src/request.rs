//! Next-request assembly: the registry's cached tool specs plus the
//! session transcript, with tool results older than the result window
//! riding as size stubs and assistant messages kept verbatim so
//! replayed native blocks ride their turns unchanged.

use craft_core::types::{ModelRequest, ToolSpec};
use craft_tools::Registry;

use crate::runner::RunConfig;
use crate::session::Session;
use crate::trim;

/// The request for the next model step: the system prompt, the trimmed
/// transcript, the cached tool specs, and the per-request output cap.
pub(crate) fn build(
    config: &RunConfig,
    registry: &Registry,
    session: &Session,
    window: u32,
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
    ModelRequest {
        system: config.system.clone(),
        messages: trim::assemble(session.messages(), window),
        tools,
        max_tokens: config.max_tokens,
        tool_choice: None,
        sampling: None,
    }
}
