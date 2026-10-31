//! Neutral-to-wire request encoding for the Messages dialect: top-level
//! system, required max_tokens, block-shaped turns, verbatim replay of
//! native blocks, image attachments, sampling knobs, tool choice, id
//! sanitation, the family max_tokens ceiling, and the cache-breakpoint
//! pass.

use craft_core::error::ProviderError;
use craft_core::types::{
    ChatMessage, ImagePart, ModelRequest, NativeBlocks, NativeDialect, Sampling, ToolCall,
    ToolChoice, thinking_level_disabled,
};
use serde_json::Value;

use super::caching;
use super::ceiling;
use super::ids::IdMap;
use super::images;

/// Neutral request to documented messages JSON; sampling and tool
/// choice ride only when the caller asked for them.
pub(super) fn encode(request: &ModelRequest, model: &str) -> Result<Value, ProviderError> {
    let mut id_map = IdMap::new();
    let mut messages: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    for message in &request.messages {
        match message.as_ref() {
            ChatMessage::User {
                text,
                images: attached,
            } => {
                flush_results(&mut messages, &mut results);
                let content = images::with_images(text, attached)?;
                messages.push(serde_json::json!({"role": "user", "content": content}));
            }
            ChatMessage::Assistant {
                text,
                tool_calls,
                native_blocks,
            } => {
                flush_results(&mut messages, &mut results);
                messages.push(encode_assistant(
                    text,
                    tool_calls,
                    native_blocks,
                    &mut id_map,
                )?);
            }
            ChatMessage::Tool {
                call_id,
                content,
                is_error,
                images: attached,
            } => results.push(tool_result_block(
                &id_map.resolve(call_id),
                content,
                *is_error,
                attached,
            )?),
        }
    }
    flush_results(&mut messages, &mut results);
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": ceiling::effective_max_tokens(model, request.max_tokens),
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
        encode_tools(&mut body, request)?;
    }
    if let Some(sampling) = &request.sampling {
        encode_sampling(&mut body, sampling)?;
    }
    if let Some(level) = &request.thinking_level {
        encode_thinking_level(&mut body, level)?;
    }
    caching::apply(&mut body);
    Ok(body)
}

/// The thinking level on the wire: `none`/`off` disables thinking,
/// anything else rides as the output effort verbatim (known levels
/// low/medium/high/xhigh/max and arbitrary strings alike).
fn encode_thinking_level(body: &mut Value, level: &str) -> Result<(), ProviderError> {
    let map = object_mut(body)?;
    if thinking_level_disabled(level) {
        map.insert(
            String::from("thinking"),
            serde_json::json!({"type": "disabled"}),
        );
    } else {
        map.insert(
            String::from("output_config"),
            serde_json::json!({"effort": level}),
        );
    }
    Ok(())
}

/// One assistant turn: replayed native blocks stand verbatim ahead of
/// the text and tool_use blocks, per the append-only history contract.
fn encode_assistant(
    text: &str,
    tool_calls: &[ToolCall],
    native_blocks: &Option<NativeBlocks>,
    id_map: &mut IdMap,
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
        content.push(tool_use_block(call, id_map)?);
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
fn tool_use_block(call: &ToolCall, id_map: &mut IdMap) -> Result<Value, ProviderError> {
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
        "id": id_map.assign(&call.id),
        "name": call.name,
        "input": input,
    }))
}

/// Tool definitions and the choice knob, which rides only with tools.
fn encode_tools(body: &mut Value, request: &ModelRequest) -> Result<(), ProviderError> {
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
    let map = object_mut(body)?;
    map.insert(String::from("tools"), Value::Array(tools));
    if let Some(choice) = &request.tool_choice {
        let choice = match choice {
            ToolChoice::Auto => serde_json::json!({"type": "auto"}),
            ToolChoice::Any => serde_json::json!({"type": "any"}),
            ToolChoice::Tool { name } => serde_json::json!({"type": "tool", "name": name}),
        };
        map.insert(String::from("tool_choice"), choice);
    }
    Ok(())
}

/// Finite temperature and top_p as JSON numbers, and stop sequences
/// only when set; anything else keeps the provider default.
fn encode_sampling(body: &mut Value, sampling: &Sampling) -> Result<(), ProviderError> {
    for (field, value) in [
        ("temperature", sampling.temperature),
        ("top_p", sampling.top_p),
    ] {
        if let Some(value) = value {
            if !value.is_finite() {
                return Err(ProviderError::InvalidRequest {
                    detail: format!("{field} must be a finite number"),
                });
            }
            object_mut(body)?.insert(String::from(field), serde_json::json!(value));
        }
    }
    if !sampling.stop_sequences.is_empty() {
        let stops: Vec<Value> = sampling
            .stop_sequences
            .iter()
            .map(|sequence| Value::String(sequence.clone()))
            .collect();
        object_mut(body)?.insert(String::from("stop_sequences"), Value::Array(stops));
    }
    Ok(())
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
fn tool_result_block(
    call_id: &str,
    content: &str,
    is_error: bool,
    attached: &[ImagePart],
) -> Result<Value, ProviderError> {
    let mut block = serde_json::json!({
        "type": "tool_result",
        "tool_use_id": call_id,
        "content": images::with_images(content, attached)?,
    });
    if let Some(map) = block.as_object_mut().filter(|_| is_error) {
        map.insert(String::from("is_error"), Value::Bool(true));
    }
    Ok(block)
}

fn object_mut(value: &mut Value) -> Result<&mut serde_json::Map<String, Value>, ProviderError> {
    value
        .as_object_mut()
        .ok_or_else(|| ProviderError::Transport {
            detail: String::from("the encoded body must be an object"),
        })
}
