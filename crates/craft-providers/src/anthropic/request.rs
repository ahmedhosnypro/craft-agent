//! Neutral-to-wire request encoding for the Messages dialect: top-level
//! system, required max_tokens, block-shaped turns, and verbatim replay
//! of native blocks.

use craft_core::error::ProviderError;
use craft_core::types::{ChatMessage, ModelRequest, NativeBlocks, NativeDialect, ToolCall};
use serde_json::Value;

/// Neutral request to documented messages JSON. No temperature rides
/// the body; the dialect deprecated it.
pub(super) fn encode(request: &ModelRequest, model: &str) -> Result<Value, ProviderError> {
    let mut messages: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    for message in &request.messages {
        match message {
            ChatMessage::User { text } => {
                flush_results(&mut messages, &mut results);
                messages.push(serde_json::json!({"role": "user", "content": text}));
            }
            ChatMessage::Assistant {
                text,
                tool_calls,
                native_blocks,
            } => {
                flush_results(&mut messages, &mut results);
                messages.push(encode_assistant(text, tool_calls, native_blocks)?);
            }
            ChatMessage::Tool {
                call_id,
                content,
                is_error,
            } => results.push(tool_result_block(call_id, content, *is_error)),
        }
    }
    flush_results(&mut messages, &mut results);
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": request.max_tokens,
        "stream": true,
    });
    if let Some(system) = request
        .system
        .as_deref()
        .filter(|system| !system.is_empty())
    {
        object_mut(&mut body)?.insert(String::from("system"), Value::String(system.to_owned()));
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.parameters,
                })
            })
            .collect();
        let map = object_mut(&mut body)?;
        map.insert(String::from("tools"), Value::Array(tools));
    }
    Ok(body)
}

/// One assistant turn: replayed native blocks stand verbatim ahead of
/// the text and tool_use blocks, per the append-only history contract.
fn encode_assistant(
    text: &str,
    tool_calls: &[ToolCall],
    native_blocks: &Option<NativeBlocks>,
) -> Result<Value, ProviderError> {
    let mut content: Vec<Value> = Vec::new();
    if let Some(blocks) = native_blocks {
        if blocks.dialect != NativeDialect::Anthropic {
            return Err(ProviderError::InvalidRequest {
                detail: String::from(
                    "native replay blocks from another dialect cannot be re-encoded",
                ),
            });
        }
        let blocks = blocks
            .blocks
            .as_array()
            .ok_or_else(|| ProviderError::InvalidRequest {
                detail: String::from("native replay blocks must be an array of content blocks"),
            })?;
        content.extend(blocks.iter().cloned());
    }
    if !text.is_empty() {
        content.push(serde_json::json!({"type": "text", "text": text}));
    }
    for call in tool_calls {
        content.push(tool_use_block(call)?);
    }
    if content.is_empty() {
        return Err(ProviderError::InvalidRequest {
            detail: String::from("an assistant turn must carry content"),
        });
    }
    Ok(serde_json::json!({"role": "assistant", "content": content}))
}

/// One tool call: the neutral arguments text is raw JSON, so it parses
/// into the input object the wire requires.
fn tool_use_block(call: &ToolCall) -> Result<Value, ProviderError> {
    let input = if call.arguments.is_empty() {
        serde_json::json!({})
    } else {
        let parsed: Value =
            serde_json::from_str(&call.arguments).map_err(|err| ProviderError::InvalidRequest {
                detail: format!("tool arguments for {} are not JSON: {err}", call.name),
            })?;
        if !parsed.is_object() {
            return Err(ProviderError::InvalidRequest {
                detail: format!("tool arguments for {} must be an object", call.name),
            });
        }
        parsed
    };
    Ok(serde_json::json!({
        "type": "tool_use",
        "id": call.id,
        "name": call.name,
        "input": input,
    }))
}

/// Consecutive tool results ride as one user message of tool_result
/// blocks, immediately after the assistant turn they answer.
fn flush_results(messages: &mut Vec<Value>, results: &mut Vec<Value>) {
    if results.is_empty() {
        return;
    }
    let blocks = std::mem::take(results);
    messages.push(serde_json::json!({
        "role": "user",
        "content": Value::Array(blocks),
    }));
}

/// Every tool_use gets a tool_result, even a rejected or cancelled one;
/// the error flag marks rows the tool layer refused.
fn tool_result_block(call_id: &str, content: &str, is_error: bool) -> Value {
    let mut block = serde_json::json!({
        "type": "tool_result",
        "tool_use_id": call_id,
        "content": content,
    });
    if let Some(map) = block.as_object_mut().filter(|_| is_error) {
        map.insert(String::from("is_error"), Value::Bool(true));
    }
    block
}

fn object_mut(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>, ProviderError> {
    value
        .as_object_mut()
        .ok_or_else(|| ProviderError::Transport {
            detail: String::from("the encoded body must be an object"),
        })
}
