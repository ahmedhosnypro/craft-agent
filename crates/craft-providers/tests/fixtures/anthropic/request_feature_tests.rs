//! Request-encoding feature rows: the family max_tokens ceiling, cache
//! breakpoints, tool choice, and sampling knobs — asserted on the
//! captured request body.

use crate::request_tests::{body_for, rejected};
use crate::{MODEL, TestError, sample_request, tool_spec};
use craft_core::error::ProviderError;
use craft_core::types::{ChatMessage, Sampling, ToolChoice};
use serde_json::Value;

/// The documented cache-breakpoint annotation.
fn ephemeral() -> Value {
    serde_json::json!({"type": "ephemeral"})
}

#[tokio::test]
async fn max_tokens_clamps_to_the_family_ceiling() -> Result<(), TestError> {
    let rows = [
        ("claude-opus-4-5", 200_000u32, 64_000u64),
        ("claude-opus-4-1", 200_000, 32_000),
        ("claude-opus-4-6", 200_000, 128_000),
        ("claude-3-opus", 8_192, 4_096),
        ("claude-sonnet-4-6", 200_000, 128_000),
        ("claude-sonnet-4-0", 100_000, 64_000),
        ("claude-3-5-sonnet", 16_384, 8_192),
        ("claude-haiku-4-5", 70_000, 64_000),
        ("claude-haiku-3-5", 9_000, 8_192),
        ("claude-3-haiku", 9_000, 4_096),
        ("claude-opus", 200_000, 128_000),
        ("claude-future-9", 500_000, 500_000),
    ];
    for (model, requested, expected) in rows {
        let mut request = sample_request();
        request.max_tokens = requested;
        let body = body_for(request, model).await?;
        assert_eq!(body["max_tokens"], expected, "{model}");
    }
    Ok(())
}

#[tokio::test]
async fn cache_breakpoints_mark_system_last_tool_and_last_message() -> Result<(), TestError> {
    let mut request = sample_request();
    request.tools = vec![tool_spec("get_weather"), tool_spec("bash")];
    let body = body_for(request, MODEL).await?;
    assert_eq!(
        body["system"],
        serde_json::json!([{
            "type": "text",
            "text": "You are a digest agent.",
            "cache_control": ephemeral(),
        }])
    );
    let tools = body["tools"].as_array().ok_or("tools must be an array")?;
    assert!(
        tools[0].get("cache_control").is_none(),
        "only the last tool carries the breakpoint"
    );
    assert_eq!(
        tools.last().ok_or("a last tool")?["cache_control"],
        ephemeral()
    );
    let last = body["messages"]
        .as_array()
        .ok_or("messages must be an array")?
        .last()
        .ok_or("a last message")?;
    assert_eq!(
        last["content"],
        serde_json::json!([{
            "type": "text",
            "text": "Summarize the merged PRs.",
            "cache_control": ephemeral(),
        }])
    );
    Ok(())
}

#[tokio::test]
async fn breakpoints_skip_absent_tools_and_empty_trailing_content() -> Result<(), TestError> {
    let mut request = sample_request();
    request.system = None;
    request.messages.push(ChatMessage::User {
        text: String::new(),
        images: Vec::new(),
    });
    let body = body_for(request, MODEL).await?;
    assert!(body.get("system").is_none());
    assert!(body.get("tools").is_none());
    let messages = body["messages"]
        .as_array()
        .ok_or("messages must be an array")?;
    let last = messages.last().ok_or("a last message")?;
    assert_eq!(
        last["content"], "",
        "empty trailing content stays a plain string"
    );
    assert!(
        messages
            .iter()
            .all(|message| message["content"].as_array().is_none()),
        "no message carries a breakpoint block"
    );
    Ok(())
}

#[tokio::test]
async fn tool_choice_maps_and_stays_off_without_tools() -> Result<(), TestError> {
    let rows = [
        (ToolChoice::Auto, serde_json::json!({"type": "auto"})),
        (ToolChoice::Any, serde_json::json!({"type": "any"})),
        (
            ToolChoice::Tool {
                name: String::from("bash"),
            },
            serde_json::json!({"type": "tool", "name": "bash"}),
        ),
    ];
    for (choice, expected) in rows {
        let mut request = sample_request();
        request.tools = vec![tool_spec("bash")];
        request.tool_choice = Some(choice);
        let body = body_for(request, MODEL).await?;
        assert_eq!(body["tool_choice"], expected);
    }
    let mut request = sample_request();
    request.tool_choice = Some(ToolChoice::Any);
    let body = body_for(request, MODEL).await?;
    assert!(
        body.get("tool_choice").is_none(),
        "the knob rides only with tools"
    );
    Ok(())
}

#[tokio::test]
async fn sampling_encodes_finite_knobs_and_rejects_non_finite() -> Result<(), TestError> {
    let mut request = sample_request();
    request.sampling = Some(Sampling {
        temperature: Some(0.7),
        top_p: Some(0.9),
        stop_sequences: vec![String::from("END")],
    });
    let body = body_for(request, MODEL).await?;
    assert_eq!(body["temperature"], 0.7);
    assert_eq!(body["top_p"], 0.9);
    assert_eq!(body["stop_sequences"], serde_json::json!(["END"]));
    let mut request = sample_request();
    request.sampling = Some(Sampling {
        temperature: Some(0.5),
        top_p: None,
        stop_sequences: Vec::new(),
    });
    let body = body_for(request, MODEL).await?;
    assert_eq!(body["temperature"], 0.5);
    assert!(
        body.get("top_p").is_none() && body.get("stop_sequences").is_none(),
        "absent knobs keep the provider default"
    );
    for (temperature, top_p) in [
        (Some(f64::NAN), None),
        (None, Some(f64::INFINITY)),
        (Some(f64::NEG_INFINITY), None),
    ] {
        let mut request = sample_request();
        request.sampling = Some(Sampling {
            temperature,
            top_p,
            stop_sequences: Vec::new(),
        });
        let err = rejected(request).await?;
        assert!(
            matches!(err, ProviderError::InvalidRequest { .. }),
            "got {err:?}"
        );
    }
    Ok(())
}
