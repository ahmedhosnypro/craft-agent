//! Request-encoding rows for attachments and tool ids: image blocks
//! with the whitelisted media types, and the sanitized tool_use /
//! tool_result id pairing — asserted on the captured request body.

use crate::request_tests::{body_for, rejected};
use crate::{MODEL, TestError, sample_request, tool_spec};
use craft_core::error::ProviderError;
use craft_core::types::{ChatMessage, ImagePart, ModelRequest, ToolCall};
use serde_json::Value;

/// One assistant tool call and its result; both ids carry the given
/// raw forms so sanitation and pairing are observable.
fn tool_exchange(raw_call_id: &str, raw_result_id: &str) -> ModelRequest {
    ModelRequest {
        system: None,
        messages: vec![
            std::sync::Arc::new(ChatMessage::User {
                text: String::from("go"),
                images: Vec::new(),
            }),
            std::sync::Arc::new(ChatMessage::Assistant {
                text: String::from("checking"),
                tool_calls: vec![ToolCall {
                    id: String::from(raw_call_id),
                    name: String::from("bash"),
                    arguments: String::from("{}"),
                }],
                native_blocks: None,
            }),
            std::sync::Arc::new(ChatMessage::Tool {
                call_id: String::from(raw_result_id),
                content: String::from("ok"),
                is_error: false,
                images: Vec::new(),
            }),
        ],
        tools: vec![tool_spec("bash")],
        max_tokens: 128,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    }
}

/// Every tool_use id and tool_result tool_use_id on the wire, in order.
fn wire_ids(body: &Value) -> Result<Vec<String>, TestError> {
    let mut ids = Vec::new();
    let messages = body["messages"]
        .as_array()
        .ok_or("messages must be an array")?;
    for message in messages {
        let Some(blocks) = message["content"].as_array() else {
            continue;
        };
        for block in blocks {
            if let Some(id) = block["id"]
                .as_str()
                .or_else(|| block["tool_use_id"].as_str())
            {
                ids.push(String::from(id));
            }
        }
    }
    Ok(ids)
}

#[tokio::test]
async fn images_ride_user_and_tool_result_content() -> Result<(), TestError> {
    let mut request = sample_request();
    request.messages[0] = std::sync::Arc::new(ChatMessage::User {
        text: String::from("Read this."),
        images: vec![
            ImagePart::Base64 {
                media_type: String::from("image/png"),
                data: String::from("aGk="),
            },
            ImagePart::Url {
                url: String::from("https://example.test/cat.png"),
            },
        ],
    });
    request
        .messages
        .push(std::sync::Arc::new(ChatMessage::Assistant {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: String::from("toolu_a"),
                name: String::from("bash"),
                arguments: String::from("{}"),
            }],
            native_blocks: None,
        }));
    request
        .messages
        .push(std::sync::Arc::new(ChatMessage::Tool {
            call_id: String::from("toolu_a"),
            content: String::from("0 files"),
            is_error: true,
            images: vec![ImagePart::Base64 {
                media_type: String::from("image/jpeg"),
                data: String::from("eA=="),
            }],
        }));
    let body = body_for(request, MODEL).await?;
    let messages = body["messages"]
        .as_array()
        .ok_or("messages must be an array")?;
    assert_eq!(
        messages[0]["content"],
        serde_json::json!([
            {"type": "text", "text": "Read this."},
            {
                "type": "image",
                "source": {"type": "base64", "media_type": "image/png", "data": "aGk="}
            },
            {"type": "image", "source": {"type": "url", "url": "https://example.test/cat.png"}},
        ])
    );
    let result = &messages[2]["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["is_error"], true);
    assert_eq!(
        result["content"],
        serde_json::json!([
            {"type": "text", "text": "0 files"},
            {
                "type": "image",
                "source": {"type": "base64", "media_type": "image/jpeg", "data": "eA=="}
            },
        ])
    );
    Ok(())
}

#[tokio::test]
async fn unsupported_image_media_types_reject_the_request() -> Result<(), TestError> {
    let mut request = sample_request();
    request.messages[0] = std::sync::Arc::new(ChatMessage::User {
        text: String::from("Read this."),
        images: vec![ImagePart::Base64 {
            media_type: String::from("image/svg+xml"),
            data: String::from("aGk="),
        }],
    });
    let err = rejected(request).await?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "got {err:?}"
    );
    assert!(crate::detail_of(&err).contains("image/svg+xml"));
    Ok(())
}

#[tokio::test]
async fn hostile_tool_ids_sanitize_and_results_pair() -> Result<(), TestError> {
    let body = body_for(tool_exchange("toolu/a b", "toolu/a b"), MODEL).await?;
    assert_eq!(
        wire_ids(&body)?,
        vec![String::from("toolu-a-b"), String::from("toolu-a-b")]
    );
    Ok(())
}

#[tokio::test]
async fn tool_ids_truncate_at_sixty_four_bytes() -> Result<(), TestError> {
    let long = format!("toolu_{}", "a".repeat(70));
    let body = body_for(tool_exchange(&long, &long), MODEL).await?;
    assert_eq!(
        wire_ids(&body)?,
        vec![long[..64].to_owned(), long[..64].to_owned()]
    );
    Ok(())
}

#[tokio::test]
async fn colliding_truncated_ids_suffix_and_results_pair() -> Result<(), TestError> {
    let base = "a".repeat(64);
    let longer = "a".repeat(70);
    let mut request = tool_exchange(&base, &base);
    if let Some(ChatMessage::Assistant { tool_calls, .. }) =
        std::sync::Arc::get_mut(&mut request.messages[1])
    {
        tool_calls.push(ToolCall {
            id: longer.clone(),
            name: String::from("bash"),
            arguments: String::from("{}"),
        });
    }
    request
        .messages
        .push(std::sync::Arc::new(ChatMessage::Tool {
            call_id: longer,
            content: String::from("also"),
            is_error: false,
            images: Vec::new(),
        }));
    let body = body_for(request, MODEL).await?;
    let second = format!("{}-2", "a".repeat(62));
    assert_eq!(
        wire_ids(&body)?,
        vec![base.clone(), second.clone(), base, second]
    );
    Ok(())
}
