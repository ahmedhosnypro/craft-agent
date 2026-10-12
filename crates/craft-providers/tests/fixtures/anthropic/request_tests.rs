//! Request encoding: the neutral request maps to the documented
//! Messages JSON — top-level system, required max_tokens, Bearer auth,
//! the version header, and no temperature.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{KEY, MODEL, TestError, provider, replay_request, run, sample_request, tool_spec};
use craft_core::error::ProviderError;
use craft_core::types::{ChatMessage, NativeBlocks, NativeDialect};
use serde_json::Value;

async fn serve_stream() -> Result<MockServer, TestError> {
    let server = MockServer::start(
        vec![CannedResponse::sse(wire::text_stream().as_bytes())],
        "",
    )
    .await?;
    Ok(server)
}

async fn captured_body(request_body: String) -> Result<Value, TestError> {
    Ok(serde_json::from_str(&request_body)?)
}

#[tokio::test]
async fn request_carries_path_bearer_version_and_documented_body() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert!(
        !events.is_empty(),
        "the served stream must produce events: {events:?}"
    );
    let request = server.request(0).ok_or("the adapter must send a request")?;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(
        request.headers.get("authorization"),
        Some(&format!("Bearer {KEY}"))
    );
    assert_eq!(
        request.headers.get("anthropic-version"),
        Some(&String::from("2023-06-01"))
    );
    assert_eq!(
        request.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    let body = captured_body(request.body).await?;
    assert_eq!(
        body,
        serde_json::json!({
            "model": MODEL,
            "system": "You are a digest agent.",
            "messages": [
                {"role": "user", "content": "Summarize the merged PRs."}
            ],
            "max_tokens": 128,
            "stream": true
        }),
        "exact body equality holds temperature and stray fields out"
    );
    Ok(())
}

#[tokio::test]
async fn tools_encode_with_input_schema() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.tools = vec![tool_spec("get_weather"), tool_spec("bash")];
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    let tools = body["tools"].as_array().ok_or("tools must be an array")?;
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "get_weather");
    assert_eq!(tools[0]["description"], "get_weather tool for fixtures");
    assert_eq!(
        tools[0]["input_schema"],
        tool_spec("get_weather").parameters
    );
    assert_eq!(tools[1]["name"], "bash");
    Ok(())
}

#[tokio::test]
async fn absent_tools_omit_the_tool_fields() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
    Ok(())
}

#[tokio::test]
async fn required_max_tokens_rides_every_request() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.max_tokens = 512;
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(body["max_tokens"], 512, "max_tokens is required");
    Ok(())
}

#[tokio::test]
async fn tool_results_pair_with_every_tool_use_even_rejected() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, replay_request()).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    let messages = body["messages"].as_array().ok_or("messages array")?;
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .ok_or("the assistant turn must replay")?;
    let content = assistant["content"].as_array().ok_or("content array")?;
    let tool_use = content
        .iter()
        .find(|block| block["type"] == "tool_use")
        .ok_or("the assistant tool_use block must replay")?;
    assert_eq!(tool_use["id"], "toolu_a");
    assert_eq!(tool_use["name"], "bash");
    assert_eq!(tool_use["input"], serde_json::json!({"cmd": "ls"}));
    let result = messages
        .iter()
        .flat_map(|message| {
            message["content"]
                .as_array()
                .map(|blocks| blocks.to_vec())
                .unwrap_or_default()
        })
        .find(|block| block["type"] == "tool_result")
        .ok_or("every tool_use gets a tool_result, even a rejected one")?;
    assert_eq!(result["tool_use_id"], "toolu_a");
    assert_eq!(result["content"], "0 files");
    assert_eq!(result["is_error"], true);
    Ok(())
}

#[tokio::test]
async fn native_thinking_blocks_replay_verbatim() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.messages.push(ChatMessage::Assistant {
        text: String::from("Weighing it."),
        tool_calls: Vec::new(),
        native_blocks: Some(NativeBlocks {
            dialect: NativeDialect::Anthropic,
            blocks: serde_json::json!([{
                "type": "thinking",
                "thinking": "the digest needs the PR titles",
                "signature": "EqMC1"
            }]),
        }),
    });
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    let assistant = body["messages"][1]
        .as_object()
        .ok_or("the assistant turn must encode")?;
    let content = assistant["content"].as_array().ok_or("content array")?;
    assert_eq!(
        content[0],
        serde_json::json!({
            "type": "thinking",
            "thinking": "the digest needs the PR titles",
            "signature": "EqMC1"
        })
    );
    assert_eq!(content[1]["type"], "text");
    Ok(())
}

#[tokio::test]
async fn foreign_native_blocks_are_rejected_before_any_request() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.messages.push(ChatMessage::Assistant {
        text: String::new(),
        tool_calls: Vec::new(),
        native_blocks: Some(NativeBlocks {
            dialect: NativeDialect::Gemini,
            blocks: serde_json::json!([{"type": "thought", "thought": "x"}]),
        }),
    });
    let events = run(&provider(&server.base_url)?, request).await;
    let err = crate::surfaced(&events)?;
    assert!(
        matches!(err, ProviderError::InvalidRequest { .. }),
        "got {err:?}"
    );
    assert_eq!(server.request_count(), 0, "no request leaves the adapter");
    Ok(())
}
