//! Recorded generateContent wire shapes: SSE `data:` JSON chunks and
//! error envelopes, composed into streams the corpus serves off loopback.

/// Joins events into one SSE body: a blank line between events and a
/// terminating one after the last.
pub fn stream(events: &[&str]) -> String {
    let mut body = events.join("\n\n");
    body.push_str("\n\n");
    body
}

const EV_HEL: &str = r#"data: {"candidates":[{"content":{"parts":[{"text":"Hel"}],"role":"model"},"index":0}],"usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":1,"totalTokenCount":12},"modelVersion":"gemini-2.5-flash","responseId":"r1"}"#;
const EV_LO: &str = r#"data: {"candidates":[{"content":{"parts":[{"text":"lo "}],"role":"model"},"index":0}],"modelVersion":"gemini-2.5-flash"}"#;
const EV_PING: &str = ": keep-alive";
const EV_WORLD: &str = r#"data: {"candidates":[{"content":{"parts":[{"text":"world"}],"role":"model"},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":2,"thoughtsTokenCount":4,"cachedContentTokenCount":3,"totalTokenCount":20},"modelVersion":"gemini-2.5-flash"}"#;

/// Text deltas, a comment line, cumulative usage, and the terminal chunk.
pub fn text_stream() -> String {
    stream(&[EV_HEL, EV_LO, EV_PING, EV_WORLD])
}

/// The same body behind a UTF-8 BOM.
pub fn bom_text_stream() -> String {
    format!("\u{feff}{}", text_stream())
}

const EV_THINK_A: &str = r#"data: {"candidates":[{"content":{"parts":[{"thought":true,"text":"pondering"},{"text":"Hel"}],"role":"model"},"index":0}],"usageMetadata":{"promptTokenCount":9,"candidatesTokenCount":1,"thoughtsTokenCount":4,"totalTokenCount":14}}"#;
const EV_THINK_B: &str = r#"data: {"candidates":[{"content":{"parts":[{"thought":true,"text":"still pondering"},{"thoughtSignature":"c2lnbmF0dXJl"},{"text":"lo"}],"role":"model"},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":9,"candidatesTokenCount":2,"thoughtsTokenCount":5,"totalTokenCount":16}}"#;

/// Thought parts and a bare thought-signature part never reach the turn
/// text; their token counts ride the reasoning bucket.
pub fn reasoning_stream() -> String {
    stream(&[EV_THINK_A, EV_THINK_B])
}

const EV_CALL_A: &str = r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"id":"call_a","name":"get_weather","args":{"city":"Paris"}}}],"role":"model"},"index":0}],"usageMetadata":{"promptTokenCount":42,"candidatesTokenCount":1,"totalTokenCount":43}}"#;
const EV_CALL_B: &str = r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"bash","args":{"cmd":"ls"}}}],"role":"model"},"index":0}],"modelVersion":"gemini-2.5-flash"}"#;
const EV_CALL_SIG: &str = r#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"id":"call_sig","name":"lookup","args":{"city":"Oslo"}},"thoughtSignature":"c2ln"}],"role":"model"},"index":0}],"modelVersion":"gemini-2.5-flash"}"#;
const EV_FINISH_STOP: &str = r#"data: {"candidates":[{"content":{"parts":[],"role":"model"},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":42,"candidatesTokenCount":9,"totalTokenCount":51},"modelVersion":"gemini-2.5-flash"}"#;

/// Two complete function calls (one with a wire id, one without) in the
/// documented whole-part form, then the terminal chunk.
pub fn tool_stream() -> String {
    stream(&[EV_CALL_A, EV_CALL_B, EV_FINISH_STOP])
}

/// A signature-bearing function call: the signature is tolerated and the
/// call underneath still folds.
pub fn signature_tool_stream() -> String {
    stream(&[EV_CALL_SIG, EV_FINISH_STOP])
}

const EV_USAGE_TAIL: &str = r#"data: {"usageMetadata":{"promptTokenCount":14,"candidatesTokenCount":3,"totalTokenCount":17},"modelVersion":"gemini-2.5-flash"}"#;

/// A finish chunk followed by a usage-only accounting frame: one
/// terminal, usage merged per bucket.
pub fn accounting_frame_stream() -> String {
    stream(&[EV_HEL, EV_LO, EV_WORLD, EV_USAGE_TAIL])
}

/// One content-free terminal chunk carrying the given finish reason.
pub fn finish_stream(reason: &str) -> String {
    let chunk = format!(
        r#"data: {{"candidates":[{{"content":{{"parts":[],"role":"model"}},"finishReason":"{reason}","index":0}}],"usageMetadata":{{"promptTokenCount":5,"candidatesTokenCount":0,"totalTokenCount":5}}}}"#
    );
    stream(&[&chunk])
}

/// The zero-content terminal stop: a fold with nothing in it.
pub fn zero_content_stream() -> String {
    finish_stream("STOP")
}

/// A blocked prompt: no candidates, promptFeedback carries the reason.
pub fn prompt_feedback_stream() -> String {
    let chunk = r#"data: {"promptFeedback":{"blockReason":"SAFETY","safetyRatings":[]},"usageMetadata":{"promptTokenCount":5,"totalTokenCount":5}}"#;
    stream(&[chunk])
}

/// A text delta, a finish chunk with no usage, and a clean close.
pub fn no_usage_stream() -> String {
    let hi =
        r#"data: {"candidates":[{"content":{"parts":[{"text":"hi"}],"role":"model"},"index":0}]}"#;
    let cut = r#"data: {"candidates":[{"content":{"parts":[],"role":"model"},"finishReason":"MAX_TOKENS","index":0}]}"#;
    stream(&[hi, cut])
}

const EV_HI: &str =
    r#"data: {"candidates":[{"content":{"parts":[{"text":"hi"}],"role":"model"},"index":0}]}"#;
const EV_STOP: &str = r#"data: {"candidates":[{"content":{"parts":[],"role":"model"},"finishReason":"STOP","index":0}]}"#;
const EV_LENGTH: &str = r#"data: {"candidates":[{"content":{"parts":[],"role":"model"},"finishReason":"MAX_TOKENS","index":0}]}"#;
const EV_LATE: &str =
    r#"data: {"candidates":[{"content":{"parts":[{"text":"late"}],"role":"model"},"index":0}]}"#;

/// Two finish chunks that disagree on the reason.
pub fn conflicting_finish_stream() -> String {
    stream(&[EV_HI, EV_STOP, EV_LENGTH])
}

/// Content that arrives after the turn already finished.
pub fn late_content_stream() -> String {
    stream(&[EV_HI, EV_STOP, EV_LATE])
}

/// A content chunk and a clean close with no finish chunk.
pub fn unfinished_stream() -> String {
    stream(&[EV_HI])
}

/// Properly framed SSE whose data is a truncated JSON fragment.
pub fn truncated_stream() -> String {
    String::from("data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"\n\n")
}

/// Event bytes that are not valid UTF-8.
pub const INVALID_UTF8_STREAM: &[u8] =
    b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"\xFF\xFF\"}]},\"index\":0}]}\n\n";

/// One content chunk with an oversized payload for the fold caps.
pub fn oversized_chunk(content: &str) -> String {
    format!(
        r#"data: {{"candidates":[{{"content":{{"parts":[{{"text":"{content}"}}],"role":"model"}},"index":0}}]}}"#
    )
}

/// A function call whose serialized args exceed the fold cap.
pub fn oversized_args_chunk(blob: &str) -> String {
    format!(
        r#"data: {{"candidates":[{{"content":{{"parts":[{{"functionCall":{{"name":"blob","args":{{"payload":"{blob}"}}}}}}],"role":"model"}},"finishReason":"STOP","index":0}}]}}"#
    )
}

/// HTTP error bodies in the documented error envelope.
pub const INVALID_BODY: &str = r#"{"error":{"code":400,"message":"Invalid value at 'generation_config.max_output_tokens': typed_value","status":"INVALID_ARGUMENT"}}"#;
pub const KEY_BODY: &str = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"FAILED_PRECONDITION"}}"#;
pub const AUTH_BODY: &str = r#"{"error":{"code":401,"message":"Request had invalid authentication credentials. Expected OAuth 2 access token.","status":"UNAUTHENTICATED"}}"#;
pub const FORBIDDEN_BODY: &str = r#"{"error":{"code":403,"message":"Method doesn't allow unregistered callers (callers without established identity).","status":"PERMISSION_DENIED"}}"#;
pub const RATE_BODY: &str = r#"{"error":{"code":429,"message":"Resource has been exhausted (e.g. check quota).","status":"RESOURCE_EXHAUSTED"}}"#;
pub const SERVER_BODY: &str = r#"{"error":{"code":500,"message":"An internal error has occurred. Please retry.","status":"INTERNAL"}}"#;
pub const NOT_FOUND_BODY: &str =
    r#"{"error":{"code":404,"message":"Model `gemini-x` does not exist.","status":"NOT_FOUND"}}"#;
pub const CONTEXT_BODY: &str = r#"{"error":{"code":400,"message":"The input token count exceeds the maximum number of tokens allowed by the model's context window.","status":"INVALID_ARGUMENT"}}"#;
