//! Request knobs: tool_choice and sampling round-trip, and the knob-less
//! request keeps its serialized shape byte-identical.

use craft_core::types::{ModelRequest, Sampling, ToolChoice};

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
fn request_without_knobs_serializes_byte_identical_to_the_old_shape()
-> Result<(), serde_json::Error> {
    let request = ModelRequest {
        system: None,
        messages: Vec::new(),
        tools: Vec::new(),
        max_tokens: 64,
        tool_choice: None,
        sampling: None,
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
    };
    let json = serde_json::to_string(&request)?;
    assert!(json.contains("\"tool_choice\""), "got {json}");
    assert!(json.contains("\"sampling\""), "got {json}");
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
    Ok(())
}
