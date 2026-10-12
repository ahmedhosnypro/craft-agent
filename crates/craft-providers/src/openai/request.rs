//! Neutral-to-wire request encoding for the Chat Completions dialect.

use craft_core::error::ProviderError;
use craft_core::types::{ChatMessage, ModelRequest};
use serde_json::Value;

/// Neutral request to documented chat/completions JSON. Replay blocks
/// from another dialect are rejected, never re-encoded.
pub(super) fn encode(request: &ModelRequest, model: &str) -> Result<Value, ProviderError> {
    let mut messages = Vec::new();
    if let Some(system) = request
        .system
        .as_deref()
        .filter(|system| !system.is_empty())
    {
        messages.push(serde_json::json!({"role": "system", "content": system}));
    }
    for message in &request.messages {
        messages.push(encode_message(message)?);
    }
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_completion_tokens": request.max_tokens,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    },
                })
            })
            .collect();
        let map = object_mut(&mut body)?;
        map.insert(String::from("tools"), Value::Array(tools));
        map.insert(
            String::from("tool_choice"),
            Value::String(String::from("auto")),
        );
    }
    Ok(body)
}

/// Neutral message to one wire message. The neutral tool-result error
/// flag has no wire form in this dialect, so it rides inside the content
/// the runtime composed.
fn encode_message(message: &ChatMessage) -> Result<Value, ProviderError> {
    match message {
        ChatMessage::User { text } => Ok(serde_json::json!({"role": "user", "content": text})),
        ChatMessage::Assistant {
            text,
            tool_calls,
            native_blocks,
        } => {
            if native_blocks.is_some() {
                return Err(ProviderError::InvalidRequest {
                    detail: String::from(
                        "native replay blocks cannot be re-encoded for the openai dialect",
                    ),
                });
            }
            let content = if text.is_empty() {
                Value::Null
            } else {
                Value::String(text.clone())
            };
            let mut value = serde_json::json!({"role": "assistant", "content": content});
            if !tool_calls.is_empty() {
                let calls: Vec<Value> = tool_calls
                    .iter()
                    .map(|call| {
                        serde_json::json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments,
                            },
                        })
                    })
                    .collect();
                object_mut(&mut value)?.insert(String::from("tool_calls"), Value::Array(calls));
            }
            Ok(value)
        }
        ChatMessage::Tool {
            call_id, content, ..
        } => Ok(serde_json::json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": content,
        })),
    }
}

fn object_mut(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>, ProviderError> {
    value
        .as_object_mut()
        .ok_or_else(|| ProviderError::Transport {
            detail: String::from("the encoded body must be an object"),
        })
}
