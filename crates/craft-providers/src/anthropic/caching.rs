//! Prompt-cache breakpoints as a final pass over the encoded body:
//! the system block, the last tool definition, and the last content
//! block of the last message.

use serde_json::Value;

/// Applies the three breakpoints to the encoded body.
pub(super) fn apply(body: &mut Value) {
    mark_system(body);
    mark_last_tool(body);
    mark_last_message(body);
}

/// Breakpoint 1: the system string becomes one marked text block.
fn mark_system(body: &mut Value) {
    let Some(system) = body.get_mut("system") else {
        return;
    };
    if !system.is_string() {
        return;
    }
    let text = system.as_str().unwrap_or_default().to_owned();
    *system = serde_json::json!([{
        "type": "text",
        "text": text,
        "cache_control": {"type": "ephemeral"},
    }]);
}

/// Breakpoint 2: the last tool definition.
fn mark_last_tool(body: &mut Value) {
    let Some(last) = body
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .and_then(|tools| tools.last_mut())
    else {
        return;
    };
    if let Some(map) = last.as_object_mut() {
        set_ephemeral(map);
    }
}

/// Breakpoint 3: the last content block of the last message, promoted
/// from a plain string when needed; empty content and unmarkable
/// blocks carry no breakpoint.
fn mark_last_message(body: &mut Value) {
    let Some(content) = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .and_then(|messages| messages.last_mut())
        .and_then(|last| last.get_mut("content"))
    else {
        return;
    };
    if content.is_string() {
        let text = content.as_str().unwrap_or_default().to_owned();
        if text.is_empty() {
            return;
        }
        *content = serde_json::json!([{
            "type": "text",
            "text": text,
            "cache_control": {"type": "ephemeral"},
        }]);
        return;
    }
    if let Some(block) = content.as_array_mut().and_then(|blocks| blocks.last_mut()) {
        mark_block(block);
    }
}

/// Only the content-block types the API accepts, and never twice.
fn mark_block(block: &mut Value) {
    let markable = block
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "text" | "image" | "tool_result" | "tool_use"));
    if !markable {
        return;
    }
    if let Some(map) = block.as_object_mut() {
        set_ephemeral(map);
    }
}

/// Sets the breakpoint unless one already rides the block.
fn set_ephemeral(map: &mut serde_json::Map<String, Value>) {
    if !map.contains_key("cache_control") {
        map.insert(
            String::from("cache_control"),
            serde_json::json!({"type": "ephemeral"}),
        );
    }
}
