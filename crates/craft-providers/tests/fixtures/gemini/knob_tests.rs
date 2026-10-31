//! Request knobs: sampling, tool choice, and the thinking request on
//! the generateContent wire, each riding only when the caller set it.

use crate::mock::{CannedResponse, MockServer};
use crate::wire;
use crate::{TestError, provider, run, sample_request, tool_spec};
use craft_core::error::ProviderError;
use craft_core::types::{ModelRequest, Sampling, Thinking, ToolChoice};
use serde_json::Value;

/// The encoded body of the one request a turn sends, served a plain
/// text stream.
async fn body_for(request: ModelRequest) -> Result<Value, TestError> {
    let server = MockServer::start(
        vec![CannedResponse::sse(wire::text_stream().as_bytes())],
        "",
    )
    .await?;
    let events = run(&provider(&server.base_url)?, request).await;
    assert!(!events.is_empty(), "the served stream must produce events");
    let sent = server
        .request(0)
        .ok_or("the adapter must send a request")?
        .body;
    Ok(serde_json::from_str(&sent)?)
}
#[tokio::test]
async fn sampling_knobs_ride_generation_config() -> Result<(), TestError> {
    let mut request = sample_request();
    request.sampling = Some(Sampling {
        temperature: Some(0.7),
        top_p: Some(0.9),
        stop_sequences: vec![String::from("END")],
    });
    let body = body_for(request).await?;
    assert_eq!(
        body["generationConfig"],
        serde_json::json!({
            "maxOutputTokens": 128,
            "temperature": 0.7,
            "topP": 0.9,
            "stopSequences": ["END"],
        })
    );
    Ok(())
}

#[tokio::test]
async fn unset_sampling_knobs_keep_the_provider_default() -> Result<(), TestError> {
    let mut request = sample_request();
    request.sampling = Some(Sampling {
        temperature: Some(0.5),
        top_p: None,
        stop_sequences: Vec::new(),
    });
    let body = body_for(request).await?;
    assert_eq!(
        body["generationConfig"],
        serde_json::json!({"maxOutputTokens": 128, "temperature": 0.5}),
        "absent knobs ride nothing"
    );
    Ok(())
}
#[tokio::test]
async fn non_finite_sampling_is_rejected_before_the_wire() -> Result<(), TestError> {
    for (temperature, top_p) in [
        (Some(f64::NAN), None),
        (None, Some(f64::INFINITY)),
        (Some(f64::NEG_INFINITY), None),
    ] {
        let server = MockServer::start(Vec::new(), "").await?;
        let mut request = sample_request();
        request.sampling = Some(Sampling {
            temperature,
            top_p,
            stop_sequences: Vec::new(),
        });
        let events = run(&provider(&server.base_url)?, request).await;
        assert_eq!(server.request_count(), 0, "no request leaves the adapter");
        assert!(
            matches!(
                events.first(),
                Some(Err(ProviderError::InvalidRequest { .. }))
            ),
            "a non-finite knob must fail typed: {events:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn tool_choice_encodes_the_function_calling_config() -> Result<(), TestError> {
    let rows = [
        (ToolChoice::Auto, serde_json::json!({"mode": "AUTO"})),
        (ToolChoice::Any, serde_json::json!({"mode": "ANY"})),
        (
            ToolChoice::Tool {
                name: String::from("bash"),
            },
            serde_json::json!({"mode": "ANY", "allowedFunctionNames": ["bash"]}),
        ),
    ];
    for (choice, expected) in rows {
        let mut request = sample_request();
        request.tools = vec![tool_spec("bash")];
        request.tool_choice = Some(choice);
        let body = body_for(request).await?;
        assert_eq!(body["toolConfig"]["functionCallingConfig"], expected);
    }
    Ok(())
}
#[tokio::test]
async fn tool_config_stays_off_without_tools_or_choice() -> Result<(), TestError> {
    let mut request = sample_request();
    request.tool_choice = Some(ToolChoice::Any);
    let body = body_for(request).await?;
    assert!(
        body.get("toolConfig").is_none(),
        "the knob rides only with tools"
    );
    let mut request = sample_request();
    request.tools = vec![tool_spec("bash")];
    let body = body_for(request).await?;
    assert!(
        body.get("toolConfig").is_none(),
        "no choice asked, the provider default rides"
    );
    Ok(())
}

#[tokio::test]
async fn thinking_encodes_the_thinking_config() -> Result<(), TestError> {
    let mut request = sample_request();
    request.thinking = Some(Thinking {
        budget_tokens: 2048,
        include_thoughts: false,
    });
    let body = body_for(request).await?;
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        serde_json::json!({"thinkingBudget": 2048})
    );
    let mut request = sample_request();
    request.thinking = Some(Thinking {
        budget_tokens: 512,
        include_thoughts: true,
    });
    let body = body_for(request).await?;
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        serde_json::json!({"thinkingBudget": 512, "includeThoughts": true}),
        "surfaced thoughts ride only when asked for"
    );
    Ok(())
}

#[tokio::test]
async fn thinking_level_rides_the_thinking_config() -> Result<(), TestError> {
    for (level, expected) in [
        ("low", serde_json::json!({"thinkingLevel": "LOW"})),
        ("high", serde_json::json!({"thinkingLevel": "HIGH"})),
        (
            "custom-string",
            serde_json::json!({"thinkingLevel": "custom-string"}),
        ),
        ("none", serde_json::json!({"thinkingBudget": 0})),
        ("off", serde_json::json!({"thinkingBudget": 0})),
    ] {
        let mut request = sample_request();
        request.thinking_level = Some(String::from(level));
        let body = body_for(request).await?;
        assert_eq!(
            body["generationConfig"]["thinkingConfig"], expected,
            "level {level}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn thinking_level_wins_over_the_thinking_struct() -> Result<(), TestError> {
    let mut request = sample_request();
    request.thinking = Some(Thinking {
        budget_tokens: 2048,
        include_thoughts: true,
    });
    request.thinking_level = Some(String::from("low"));
    let body = body_for(request).await?;
    assert_eq!(
        body["generationConfig"]["thinkingConfig"],
        serde_json::json!({"thinkingLevel": "LOW"}),
        "the level replaces the budget form entirely"
    );
    Ok(())
}

#[tokio::test]
async fn unset_thinking_level_omits_the_thinking_config() -> Result<(), TestError> {
    let body = body_for(sample_request()).await?;
    assert!(body["generationConfig"].get("thinkingConfig").is_none());
    Ok(())
}
