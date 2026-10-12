//! Neutral-to-wire request encoding for the generateContent dialect.

use craft_core::error::ProviderError;
use craft_core::types::{ChatMessage, ModelRequest, NativeDialect};
use serde_json::Value;

/// Neutral request to documented generateContent JSON. Native replay
/// blocks from this dialect re-ride verbatim; another dialect is a typed
/// rejection, never a re-encode.
pub(super) fn encode(request: &ModelRequest) -> Result<Value, ProviderError> {
    let mut contents = Vec::new();
    for (index, message) in request.messages.iter().enumerate() {
        contents.push(encode_message(message, &request.messages[..index])?);
    }
    let mut body = serde_json::json!({
        "contents": contents,
        "generationConfig": {"maxOutputTokens": request.max_tokens},
    });
    if let Some(system) = request
        .system
        .as_deref()
        .filter(|system| !system.is_empty())
    {
        let map = object_mut(&mut body)?;
        map.insert(
            String::from("systemInstruction"),
            serde_json::json!({"parts": [{"text": system}]}),
        );
    }
    if !request.tools.is_empty() {
        let declarations: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parametersJsonSchema": tool.parameters,
                })
            })
            .collect();
        let map = object_mut(&mut body)?;
        map.insert(
            String::from("tools"),
            serde_json::json!([{"functionDeclarations": declarations}]),
        );
    }
    Ok(body)
}

/// One neutral message to one wire content. The neutral tool-result
/// error flag rides the response object's documented error key.
fn encode_message(message: &ChatMessage, prior: &[ChatMessage]) -> Result<Value, ProviderError> {
    match message {
        ChatMessage::User { text, .. } => Ok(serde_json::json!({
            "role": "user",
            "parts": [{"text": text}]
        })),
        ChatMessage::Assistant {
            text,
            tool_calls,
            native_blocks,
        } => {
            let mut parts = Vec::new();
            if let Some(blocks) = native_blocks {
                parts.extend(native_parts(blocks)?);
            }
            if !text.is_empty() {
                parts.push(serde_json::json!({"text": text}));
            }
            for call in tool_calls {
                let mut function = serde_json::Map::new();
                if !call.id.is_empty() {
                    function.insert(String::from("id"), Value::String(call.id.clone()));
                }
                function.insert(String::from("name"), Value::String(call.name.clone()));
                function.insert(String::from("args"), wire_args(&call.arguments)?);
                parts.push(serde_json::json!({"functionCall": function}));
            }
            Ok(serde_json::json!({"role": "model", "parts": parts}))
        }
        ChatMessage::Tool {
            call_id,
            content,
            is_error,
            ..
        } => {
            let name = resolve_call_name(prior, call_id)?;
            let mut response = serde_json::Map::new();
            if !call_id.is_empty() {
                response.insert(String::from("id"), Value::String(call_id.clone()));
            }
            response.insert(String::from("name"), Value::String(name));
            response.insert(
                String::from("response"),
                if *is_error {
                    serde_json::json!({"error": content})
                } else {
                    wire_response(content)
                },
            );
            Ok(serde_json::json!({
                "role": "user",
                "parts": [{"functionResponse": response}]
            }))
        }
    }
}

/// Native replay parts: this dialect only, array form only, verbatim.
fn native_parts(blocks: &craft_core::types::NativeBlocks) -> Result<Vec<Value>, ProviderError> {
    if blocks.dialect != NativeDialect::Gemini {
        return Err(ProviderError::InvalidRequest {
            detail: String::from("native replay blocks belong to another dialect"),
        });
    }
    let Value::Array(parts) = &blocks.blocks else {
        return Err(ProviderError::InvalidRequest {
            detail: String::from("native replay blocks must be a parts array"),
        });
    };
    Ok(parts.clone())
}

/// functionResponse requires the function name, so the result resolves
/// it from the assistant call it answers.
fn resolve_call_name(prior: &[ChatMessage], call_id: &str) -> Result<String, ProviderError> {
    for message in prior.iter().rev() {
        if let ChatMessage::Assistant { tool_calls, .. } = message {
            for call in tool_calls {
                if call.id == call_id {
                    return Ok(call.name.clone());
                }
            }
        }
    }
    Err(ProviderError::InvalidRequest {
        detail: format!("no assistant tool call pairs with result id {call_id}"),
    })
}

fn object_mut(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>, ProviderError> {
    value
        .as_object_mut()
        .ok_or_else(|| ProviderError::Transport {
            detail: String::from("the encoded body must be an object"),
        })
}

/// A functionCall's args: the neutral arguments are raw JSON-object
/// text; the wire wants the object itself. An empty text is the empty
/// object, anything unparsable or non-object is a typed rejection.
fn wire_args(arguments: &str) -> Result<Value, ProviderError> {
    let parsed = if arguments.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(arguments).map_err(|err| ProviderError::InvalidRequest {
            detail: format!("tool arguments are not valid JSON: {err}"),
        })?
    };
    if !parsed.is_object() {
        return Err(ProviderError::InvalidRequest {
            detail: String::from("tool arguments must be a JSON object"),
        });
    }
    Ok(parsed)
}

/// The documented response object: an error result rides the error key,
/// a JSON-object result rides verbatim, any other text wraps in output.
fn wire_response(content: &str) -> Value {
    if let Ok(parsed) = serde_json::from_str::<Value>(content)
        && parsed.is_object()
    {
        return parsed;
    }
    serde_json::json!({"output": content})
}
