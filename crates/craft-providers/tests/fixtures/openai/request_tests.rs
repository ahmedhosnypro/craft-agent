//! Request encoding: the neutral request maps to the documented
//! chat/completions JSON, Bearer auth, and the versioned path.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{KEY, MODEL, TestError, provider, replay_request, run, sample_request, tool_spec};
use craft_core::error::ProviderError;
use craft_core::types::{ChatMessage, NativeBlocks, NativeDialect};
use serde_json::Value;

async fn serve_stream() -> Result<MockServer, TestError> {
    let server = MockServer::start(
        vec![CannedResponse::sse(wire::text_stream().as_bytes())],
        "/v1",
    )
    .await?;
    Ok(server)
}

async fn captured_body(request_body: String) -> Result<Value, TestError> {
    Ok(serde_json::from_str(&request_body)?)
}

#[tokio::test]
async fn request_carries_path_bearer_and_documented_body() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert!(
        !events.is_empty(),
        "the served stream must produce events: {events:?}"
    );
    let request = server.request(0).ok_or("the adapter must send a request")?;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/v1/chat/completions");
    assert_eq!(
        request.headers.get("authorization"),
        Some(&format!("Bearer {KEY}"))
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
            "messages": [
                {"role": "system", "content": "You are a digest agent."},
                {"role": "user", "content": "Summarize the merged PRs."}
            ],
            "max_completion_tokens": 128,
            "stream": true,
            "stream_options": {"include_usage": true}
        }),
        "exact body equality holds stray fields out"
    );
    Ok(())
}

#[tokio::test]
async fn tools_encode_with_auto_choice() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.tools = vec![tool_spec("get_weather"), tool_spec("bash")];
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(body["tool_choice"], "auto");
    assert_eq!(body["tools"][0]["type"], "function");
    assert_eq!(body["tools"][0]["function"]["name"], "get_weather");
    assert_eq!(
        body["tools"][0]["function"]["parameters"],
        tool_spec("get_weather").parameters
    );
    assert_eq!(body["tools"][1]["function"]["name"], "bash");
    Ok(())
}

#[tokio::test]
async fn absent_tools_omit_the_tool_fields() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert!(body.get("tools").is_none(), "no tools offered, none sent");
    assert!(body.get("tool_choice").is_none());
    Ok(())
}

#[tokio::test]
async fn assistant_tool_calls_encode_with_null_content() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, replay_request("")).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(
        body["messages"][1],
        serde_json::json!({
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_a",
                "type": "function",
                "function": {"name": "bash", "arguments": "{\"cmd\":\"ls\"}"}
            }]
        })
    );
    Ok(())
}

#[tokio::test]
async fn assistant_text_and_calls_encode_together() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, replay_request("Running it.")).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(body["messages"][1]["content"], "Running it.");
    assert_eq!(
        body["messages"][1]["tool_calls"][0]["function"]["name"],
        "bash"
    );
    Ok(())
}

#[tokio::test]
async fn tool_results_encode_with_the_dialect_flag_dropped() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, replay_request("")).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(
        body["messages"][2],
        serde_json::json!({
            "role": "tool",
            "tool_call_id": "call_a",
            "content": "0 files"
        }),
        "the neutral is_error flag has no wire form in this dialect"
    );
    Ok(())
}

#[tokio::test]
async fn foreign_native_blocks_are_rejected_before_any_request() -> Result<(), TestError> {
    let server = MockServer::start(Vec::new(), "/v1").await?;
    let mut request = sample_request();
    request
        .messages
        .push(std::sync::Arc::new(ChatMessage::Assistant {
            text: String::new(),
            tool_calls: Vec::new(),
            native_blocks: Some(NativeBlocks {
                dialect: NativeDialect::Anthropic,
                blocks: serde_json::json!([]),
            }),
        }));
    let events = run(&provider(&server.base_url)?, request).await;
    assert_eq!(
        server.request_count(),
        0,
        "the rejection must happen before the wire"
    );
    assert!(
        matches!(
            events.first(),
            Some(Err(ProviderError::InvalidRequest { .. }))
        ),
        "replay blocks from another dialect must fail typed: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn thinking_level_rides_reasoning_effort_verbatim() -> Result<(), TestError> {
    for (level, expected) in [
        ("low", "low"),
        ("high", "high"),
        ("custom-string", "custom-string"),
        ("off", "none"),
        ("Off", "none"),
    ] {
        let server = serve_stream().await?;
        let mut request = sample_request();
        request.thinking_level = Some(String::from(level));
        let events = run(&provider(&server.base_url)?, request).await;
        assert!(!events.is_empty());
        let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
        assert_eq!(body["reasoning_effort"], expected, "level {level}");
    }
    Ok(())
}

#[tokio::test]
async fn unset_thinking_level_omits_reasoning_effort() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert!(body.get("reasoning_effort").is_none());
    Ok(())
}
