//! Request encoding: the neutral request maps to the documented
//! generateContent JSON, the key header, and the versioned model path.

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
async fn request_carries_path_key_header_and_documented_body() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert!(
        !events.is_empty(),
        "the served stream must produce events: {events:?}"
    );
    let request = server.request(0).ok_or("the adapter must send a request")?;
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.path,
        format!("/v1beta/models/{MODEL}:streamGenerateContent?alt=sse")
    );
    assert_eq!(
        request.headers.get("x-goog-api-key"),
        Some(&KEY.to_owned()),
        "the documented key header carries the credential"
    );
    assert_eq!(
        request.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    let body = captured_body(request.body).await?;
    assert_eq!(
        body,
        serde_json::json!({
            "contents": [
                {"parts": [{"text": "Summarize the merged PRs."}], "role": "user"}
            ],
            "systemInstruction": {"parts": [{"text": "You are a digest agent."}]},
            "generationConfig": {"maxOutputTokens": 128}
        }),
        "exact body equality holds stray fields out"
    );
    Ok(())
}

#[tokio::test]
async fn absent_system_omits_the_field() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.system = None;
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert!(
        body.get("systemInstruction").is_none(),
        "an empty system prompt sends no systemInstruction"
    );
    Ok(())
}

#[tokio::test]
async fn tools_encode_as_function_declarations() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.tools = vec![tool_spec("get_weather"), tool_spec("bash")];
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(
        body["tools"],
        serde_json::json!([
            {"functionDeclarations": [
                {
                    "name": "get_weather",
                    "description": "get_weather tool for fixtures",
                    "parametersJsonSchema": tool_spec("get_weather").parameters,
                },
                {"name": "bash", "description": "bash tool for fixtures",
                 "parametersJsonSchema": tool_spec("bash").parameters}
            ]}
        ]),
        "the JSON-Schema parameter form rides parametersJsonSchema"
    );
    Ok(())
}

#[tokio::test]
async fn absent_tools_omit_the_field() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, sample_request()).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert!(body.get("tools").is_none(), "no tools offered, none sent");
    Ok(())
}

#[tokio::test]
async fn assistant_turn_encodes_text_and_function_calls() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, replay_request("Running it.")).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(
        body["contents"][1],
        serde_json::json!({
            "role": "model",
            "parts": [
                {"text": "Running it."},
                {"functionCall": {"id": "call_a", "name": "bash", "args": {"cmd": "ls"}}}
            ]
        })
    );
    Ok(())
}

#[tokio::test]
async fn tool_results_encode_as_function_response_parts() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let events = run(&provider(&server.base_url)?, replay_request("")).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(
        body["contents"][2],
        serde_json::json!({
            "role": "user",
            "parts": [{"functionResponse":
                {"id": "call_a", "name": "bash", "response": {"error": "0 files"}}}]
        }),
        "the result name resolves from the paired call; errors ride the error key"
    );
    Ok(())
}

#[tokio::test]
async fn json_object_results_ride_verbatim() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let mut request = sample_request();
    request.messages = vec![
        ChatMessage::Assistant {
            text: String::new(),
            tool_calls: vec![crate::tool_call(
                "call_j",
                "get_weather",
                "{\"city\":\"Oslo\"}",
            )],
            native_blocks: None,
        },
        ChatMessage::Tool {
            call_id: String::from("call_j"),
            content: String::from("{\"output\": \"3C\"}"),
            is_error: false,
        },
    ];
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(
        body["contents"][1]["parts"][0]["functionResponse"]["response"],
        serde_json::json!({"output": "3C"}),
        "a JSON-object result is the response object verbatim"
    );
    Ok(())
}

#[tokio::test]
async fn unpaired_tool_result_is_rejected_before_the_wire() -> Result<(), TestError> {
    let server = MockServer::start(Vec::new(), "").await?;
    let mut request = sample_request();
    request.messages = vec![ChatMessage::Tool {
        call_id: String::from("call_missing"),
        content: String::from("orphan"),
        is_error: false,
    }];
    let events = run(&provider(&server.base_url)?, request).await;
    assert_eq!(server.request_count(), 0, "no request leaves the adapter");
    assert!(
        matches!(
            events.first(),
            Some(Err(ProviderError::InvalidRequest { .. }))
        ),
        "a result without its call fails typed: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_arguments_are_rejected_before_the_wire() -> Result<(), TestError> {
    let server = MockServer::start(Vec::new(), "").await?;
    let mut request = sample_request();
    request.messages = vec![ChatMessage::Assistant {
        text: String::new(),
        tool_calls: vec![crate::tool_call("call_bad", "bash", "{\"cmd\"")],
        native_blocks: None,
    }];
    let events = run(&provider(&server.base_url)?, request).await;
    assert_eq!(server.request_count(), 0);
    assert!(
        matches!(
            events.first(),
            Some(Err(ProviderError::InvalidRequest { .. }))
        ),
        "tool arguments that are not a JSON object fail typed: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn gemini_native_blocks_replay_verbatim() -> Result<(), TestError> {
    let server = serve_stream().await?;
    let blocks = serde_json::json!([
        {"thought": true, "text": "pondering"},
        {"thoughtSignature": "c2lnbmF0dXJl"}
    ]);
    let mut request = sample_request();
    request.messages = vec![ChatMessage::Assistant {
        text: String::from("Done."),
        tool_calls: vec![crate::tool_call("call_a", "bash", "{\"cmd\":\"ls\"}")],
        native_blocks: Some(NativeBlocks {
            dialect: NativeDialect::Gemini,
            blocks: blocks.clone(),
        }),
    }];
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty());
    let body = captured_body(server.request(0).ok_or("a request")?.body).await?;
    assert_eq!(
        body["contents"][0]["parts"],
        serde_json::json!([
            {"thought": true, "text": "pondering"},
            {"thoughtSignature": "c2lnbmF0dXJl"},
            {"text": "Done."},
            {"functionCall": {"id": "call_a", "name": "bash", "args": {"cmd": "ls"}}}
        ]),
        "native thought-signature blocks replay verbatim ahead of the turn"
    );
    Ok(())
}

#[tokio::test]
async fn foreign_native_blocks_are_rejected_before_any_request() -> Result<(), TestError> {
    let server = MockServer::start(Vec::new(), "").await?;
    let mut request = sample_request();
    request.messages.push(ChatMessage::Assistant {
        text: String::new(),
        tool_calls: Vec::new(),
        native_blocks: Some(NativeBlocks {
            dialect: NativeDialect::Anthropic,
            blocks: serde_json::json!([]),
        }),
    });
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
