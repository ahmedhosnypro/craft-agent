//! Request knobs: tool_choice, sampling, and thinking round-trip, and
//! the knob-less request keeps its serialized shape byte-identical.

use craft_core::types::{ModelRequest, Sampling, Thinking, ToolChoice};

#[test]
fn tool_choice_variants_round_trip() -> Result<(), serde_json::Error> {
    let cases = vec![
        (ToolChoice::Auto, "\"auto\""),
        (ToolChoice::Any, "\"any\""),
        (
            ToolChoice::Tool {
                name: String::from("grep"),
            },
            r#"{"tool":{"name":"grep"}}"#,
        ),
    ];
    for (choice, shape) in cases {
        let json = serde_json::to_string(&choice)?;
        assert_eq!(json, shape);
        let back: ToolChoice = serde_json::from_str(&json)?;
        assert_eq!(back, choice);
    }
    Ok(())
}

#[test]
fn sampling_round_trips_and_omits_unset_knobs() -> Result<(), serde_json::Error> {
    let sampling = Sampling {
        temperature: Some(0.7),
        top_p: None,
        stop_sequences: vec![String::from("END")],
    };
    let json = serde_json::to_string(&sampling)?;
    assert_eq!(json, r#"{"temperature":0.7,"stop_sequences":["END"]}"#);
    let back: Sampling = serde_json::from_str(&json)?;
    assert_eq!(back, sampling);
    let bare: Sampling = serde_json::from_str("{}")?;
    assert_eq!(bare, Sampling::default());
    Ok(())
}

#[test]
fn request_without_knobs_serializes_without_knob_fields() -> Result<(), serde_json::Error> {
    let request = ModelRequest {
        system: None,
        messages: Vec::new(),
        tools: Vec::new(),
        max_tokens: 64,
        tool_choice: None,
        sampling: None,
        thinking: None,
        thinking_level: None,
    };
    let json = serde_json::to_string(&request)?;
    assert_eq!(
        json,
        r#"{"system":null,"messages":[],"tools":[],"max_tokens":64}"#
    );
    let back: ModelRequest = serde_json::from_str(&json)?;
    assert_eq!(back, request);
    Ok(())
}

#[test]
fn request_with_knobs_round_trips() -> Result<(), serde_json::Error> {
    let request = ModelRequest {
        system: Some(String::from("be terse")),
        messages: Vec::new(),
        tools: Vec::new(),
        max_tokens: 64,
        tool_choice: Some(ToolChoice::Tool {
            name: String::from("grep"),
        }),
        sampling: Some(Sampling {
            temperature: Some(0.2),
            top_p: Some(0.9),
            stop_sequences: Vec::new(),
        }),
        thinking: Some(Thinking {
            budget_tokens: 1024,
            include_thoughts: true,
        }),
        thinking_level: Some(String::from("high")),
    };
    let json = serde_json::to_string(&request)?;
    assert!(json.contains("\"tool_choice\""), "got {json}");
    assert!(json.contains("\"sampling\""), "got {json}");
    assert!(json.contains("\"thinking\""), "got {json}");
    assert!(json.contains("\"thinking_level\":\"high\""), "got {json}");
    let back: ModelRequest = serde_json::from_str(&json)?;
    assert_eq!(back, request);
    Ok(())
}

#[test]
fn requests_without_the_knob_fields_still_deserialize() -> Result<(), serde_json::Error> {
    let request: ModelRequest =
        serde_json::from_str(r#"{"system":null,"messages":[],"tools":[],"max_tokens":64}"#)?;
    assert_eq!(request.tool_choice, None);
    assert_eq!(request.sampling, None);
    assert_eq!(request.thinking, None);
    assert_eq!(request.thinking_level, None);
    Ok(())
}

#[test]
fn thinking_level_helpers_classify_levels() {
    use craft_core::types::{known_thinking_level, thinking_level_disabled};
    assert!(thinking_level_disabled("none"));
    assert!(thinking_level_disabled("OFF"));
    assert!(!thinking_level_disabled("low"));
    for level in ["low", "medium", "high", "xhigh", "max", "None", "off"] {
        assert!(known_thinking_level(level), "{level} is known");
    }
    assert!(!known_thinking_level("custom-verbosity"));
}

#[test]
fn thinking_round_trips_and_omits_the_flag_when_unset() -> Result<(), serde_json::Error> {
    let thinking = Thinking {
        budget_tokens: 2048,
        include_thoughts: false,
    };
    let json = serde_json::to_string(&thinking)?;
    assert_eq!(json, r#"{"budget_tokens":2048}"#);
    let back: Thinking = serde_json::from_str(&json)?;
    assert_eq!(back, thinking);
    let surfaced = serde_json::to_string(&Thinking {
        budget_tokens: 512,
        include_thoughts: true,
    })?;
    assert_eq!(surfaced, r#"{"budget_tokens":512,"include_thoughts":true}"#);
    let bare: Thinking = serde_json::from_str(r#"{"budget_tokens":16}"#)?;
    assert!(!bare.include_thoughts);
    Ok(())
}
