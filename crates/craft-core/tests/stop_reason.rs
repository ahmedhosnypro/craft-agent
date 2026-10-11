//! Stop-reason vocabulary: each provider dialect maps into a closed set
//! with an Other(String) fallback that absorbs unknown finish reasons
//! from OpenAI-compatible gateways.

use craft_core::types::StopReason;

#[test]
fn openai_known_finish_reasons_map_to_the_closed_set() {
    assert_eq!(StopReason::from_openai("stop"), StopReason::EndTurn);
    assert_eq!(StopReason::from_openai("length"), StopReason::MaxTokens);
    assert_eq!(StopReason::from_openai("tool_calls"), StopReason::ToolUse);
    assert_eq!(
        StopReason::from_openai("function_call"),
        StopReason::ToolUse,
        "the legacy function_call form still means the model called a tool"
    );
    assert_eq!(
        StopReason::from_openai("content_filter"),
        StopReason::ContentFilter
    );
}

#[test]
fn openai_unknown_finish_reason_is_absorbed() {
    assert_eq!(
        StopReason::from_openai("error"),
        StopReason::Other(String::from("error")),
        "extra gateway finish reasons must be absorbed, not rejected"
    );
}

#[test]
fn openai_empty_finish_reason_is_absorbed() {
    assert_eq!(
        StopReason::from_openai(""),
        StopReason::Other(String::new())
    );
}

#[test]
fn openai_matching_is_case_insensitive() {
    assert_eq!(StopReason::from_openai("STOP"), StopReason::EndTurn);
    assert_eq!(StopReason::from_openai("Tool_Calls"), StopReason::ToolUse);
}

#[test]
fn anthropic_known_stop_reasons_map_to_the_closed_set() {
    assert_eq!(StopReason::from_anthropic("end_turn"), StopReason::EndTurn);
    assert_eq!(
        StopReason::from_anthropic("max_tokens"),
        StopReason::MaxTokens
    );
    assert_eq!(
        StopReason::from_anthropic("stop_sequence"),
        StopReason::StopSequence
    );
    assert_eq!(StopReason::from_anthropic("tool_use"), StopReason::ToolUse);
    assert_eq!(
        StopReason::from_anthropic("pause_turn"),
        StopReason::PausedTurn
    );
    assert_eq!(StopReason::from_anthropic("refusal"), StopReason::Refusal);
    assert_eq!(
        StopReason::from_anthropic("model_context_window_exceeded"),
        StopReason::ContextWindowExceeded
    );
}

#[test]
fn anthropic_unknown_stop_reason_is_absorbed() {
    assert_eq!(
        StopReason::from_anthropic("server_busy"),
        StopReason::Other(String::from("server_busy"))
    );
}

#[test]
fn gemini_known_finish_reasons_map_to_the_closed_set() {
    assert_eq!(StopReason::from_gemini("STOP"), StopReason::EndTurn);
    assert_eq!(StopReason::from_gemini("MAX_TOKENS"), StopReason::MaxTokens);
    assert_eq!(StopReason::from_gemini("SAFETY"), StopReason::ContentFilter);
    assert_eq!(
        StopReason::from_gemini("RECITATION"),
        StopReason::ContentFilter
    );
    assert_eq!(
        StopReason::from_gemini("LANGUAGE"),
        StopReason::ContentFilter
    );
    assert_eq!(
        StopReason::from_gemini("BLOCKLIST"),
        StopReason::ContentFilter
    );
    assert_eq!(
        StopReason::from_gemini("PROHIBITED_CONTENT"),
        StopReason::ContentFilter
    );
    assert_eq!(StopReason::from_gemini("SPII"), StopReason::ContentFilter);
    assert_eq!(
        StopReason::from_gemini("MALFORMED_FUNCTION_CALL"),
        StopReason::MalformedToolCall
    );
    assert_eq!(
        StopReason::from_gemini("UNEXPECTED_TOOL_CALL"),
        StopReason::MalformedToolCall
    );
}

#[test]
fn gemini_matching_is_case_insensitive() {
    assert_eq!(StopReason::from_gemini("stop"), StopReason::EndTurn);
    assert_eq!(StopReason::from_gemini("max_tokens"), StopReason::MaxTokens);
}

#[test]
fn gemini_unknown_finish_reason_is_absorbed() {
    assert_eq!(
        StopReason::from_gemini("IMAGE_SAFETY"),
        StopReason::Other(String::from("IMAGE_SAFETY"))
    );
}

#[test]
fn other_preserves_the_raw_provider_string() {
    assert_eq!(
        StopReason::from_openai("Weird-Reason"),
        StopReason::Other(String::from("Weird-Reason"))
    );
}
