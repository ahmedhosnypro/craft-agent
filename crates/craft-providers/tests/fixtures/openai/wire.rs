//! Recorded Chat Completions wire shapes: SSE chunk events and error
//! bodies, composed into streams the corpus serves off loopback.

/// The SSE terminator OpenAI-compatible streams end with.
pub const DONE: &str = "data: [DONE]";

/// Joins events into one SSE body: a blank line between events and a
/// terminating one after the last.
pub fn stream(events: &[&str]) -> String {
    let mut body = events.join("\n\n");
    body.push_str("\n\n");
    body
}

const EV_HEL: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"role":"assistant","content":"Hel"},"finish_reason":null}]}"#;
const EV_LO: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"content":"lo "},"finish_reason":null}]}"#;
const EV_WORLD: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"content":"world"},"finish_reason":null}]}"#;
const EV_PING: &str = ": keep-alive";
const EV_STOP: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":null}"#;
const EV_USAGE: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":2,"total_tokens":13}}"#;

/// Text deltas, a comment line, a stop chunk, and the empty-choices
/// accounting frame the include_usage option produces.
pub fn text_stream() -> String {
    stream(&[EV_HEL, EV_LO, EV_WORLD, EV_PING, EV_STOP, EV_USAGE, DONE])
}

/// The same body behind a UTF-8 BOM.
pub fn bom_text_stream() -> String {
    format!("\u{feff}{}", text_stream())
}

const EV_HI: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}"#;
const EV_LENGTH: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{},"finish_reason":"length"}],"usage":null}"#;
const EV_LATE: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"content":"late"},"finish_reason":null}]}"#;

/// A gateway that reports no usage and never sends the terminator.
pub fn no_usage_stream() -> String {
    stream(&[EV_HI, EV_LENGTH])
}

/// A finish chunk and a clean close, with neither usage nor terminator.
pub fn bare_finish_stream() -> String {
    stream(&[EV_HI, EV_STOP])
}

/// A finish chunk with no content before it: the zero-content terminal.
pub fn zero_content_stream() -> String {
    stream(&[EV_STOP, DONE])
}

/// One content-free chunk carrying the given finish reason.
pub fn finish_stream(reason: &str) -> String {
    let chunk = format!(
        r#"data: {{"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{{"index":0,"delta":{{}},"finish_reason":"{reason}"}}]}}"#
    );
    stream(&[&chunk, DONE])
}

const EV_CALL_HEAD: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"get_weather","arguments":""}}]},"finish_reason":null}]}"#;
const EV_CALL_ARGS: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":\"Paris\"}"}}]},"finish_reason":null}]}"#;
const EV_CALL_B_HEAD: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_b","type":"function","function":{"name":"bash","arguments":"{\"cmd\":"}}]},"finish_reason":null}]}"#;
const EV_CALL_B_ARGS: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"ls\"}"}}]},"finish_reason":null}]}"#;
const EV_FINISH_TOOLS: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":null}"#;
const EV_USAGE_FULL: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[],"usage":{"prompt_tokens":42,"completion_tokens":9,"total_tokens":51,"prompt_tokens_details":{"cached_tokens":7},"completion_tokens_details":{"reasoning_tokens":4}}}"#;

/// Two tool calls assembled from index-keyed fragments, per the
/// documented accumulation pattern.
pub fn tool_stream() -> String {
    stream(&[
        EV_CALL_HEAD,
        EV_CALL_ARGS,
        EV_CALL_B_HEAD,
        EV_CALL_B_ARGS,
        EV_FINISH_TOOLS,
        EV_USAGE_FULL,
        DONE,
    ])
}

const EV_REPEAT_USAGE: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}}"#;

/// A usage chunk that repeats the finish reason with non-empty choices:
/// an accounting frame, never a second terminal.
pub fn accounting_repeat_stream() -> String {
    stream(&[EV_HI, EV_STOP, EV_REPEAT_USAGE, DONE])
}

/// Two finish chunks that disagree on the reason.
pub fn conflicting_finish_stream() -> String {
    stream(&[EV_HI, EV_STOP, EV_LENGTH, DONE])
}

/// Content that arrives after the turn already finished.
pub fn late_content_stream() -> String {
    stream(&[EV_HI, EV_STOP, EV_LATE, DONE])
}

/// The terminator with no finish chunk behind it.
pub fn done_only_stream() -> String {
    stream(&[DONE])
}

/// A content chunk and a clean close with no finish chunk.
pub fn unfinished_stream() -> String {
    stream(&[EV_HI])
}

/// Properly framed SSE whose data is a truncated JSON fragment.
pub fn truncated_stream() -> String {
    String::from("data: {\"choices\":[{\"index\":0,\"delta\":{\"cont\n\n")
}

/// Event bytes that are not valid UTF-8.
pub const INVALID_UTF8_STREAM: &[u8] =
    b"data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\xFF\xFF\"},\"finish_reason\":null}]}\n\n";

const EV_DUP_A: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"bash","arguments":""}}]},"finish_reason":null}]}"#;
const EV_DUP_Z: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_z","type":"function","function":{"name":"bash","arguments":""}}]},"finish_reason":null}]}"#;

/// Two fragments sharing a wire index with conflicting ids.
pub fn duplicate_index_stream() -> String {
    stream(&[EV_DUP_A, EV_DUP_Z, EV_FINISH_TOOLS, DONE])
}

const EV_BAD_HEAD: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_bad","type":"function","function":{"name":"lookup","arguments":"{\"city\":"}}]},"finish_reason":null}]}"#;
const EV_BAD_TAIL: &str = r#"data: {"id":"c1","object":"chat.completion.chunk","created":1760000000,"model":"gpt-5.5","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"Paris"}}]},"finish_reason":null}]}"#;

/// Argument fragments that concatenate into something that is not JSON.
pub fn bad_arguments_stream() -> String {
    stream(&[EV_BAD_HEAD, EV_BAD_TAIL, EV_FINISH_TOOLS, DONE])
}

/// One content chunk with an oversized payload for the fold caps.
pub fn oversized_chunk(content: &str) -> String {
    format!(
        r#"data: {{"choices":[{{"index":0,"delta":{{"content":"{content}"}},"finish_reason":null}}]}}"#
    )
}

/// A content chunk and then a mid-stream error envelope of the given
/// type and code.
pub fn midstream_error_stream(kind: &str, code: Option<&str>) -> String {
    let code = match code {
        Some(code) => format!("\"code\":\"{code}\""),
        None => String::from("\"code\":null"),
    };
    let chunk = format!(
        r#"data: {{"error":{{"message":"provider failed mid-stream","type":"{kind}","param":null,{code}}}}}"#
    );
    stream(&[EV_HI, &chunk])
}

/// HTTP error bodies in the documented error envelope.
pub const AUTH_BODY: &str = r#"{"error":{"message":"Incorrect API key provided: <redacted>.","type":"invalid_request_error","param":null,"code":"invalid_api_key"}}"#;
pub const FORBIDDEN_BODY: &str = r#"{"error":{"message":"Project does not have access to this model.","type":"invalid_request_error","param":null,"code":"model_not_allowed"}}"#;
pub const RATE_BODY: &str = r#"{"error":{"message":"Rate limit reached for requests.","type":"rate_limit_error","param":null,"code":null}}"#;
pub const QUOTA_BODY: &str = r#"{"error":{"message":"You exceeded your current quota, please check your plan and billing details.","type":"insufficient_quota","param":null,"code":"insufficient_quota"}}"#;
pub const SERVER_BODY: &str = r#"{"error":{"message":"The server had an error while processing your request.","type":"server_error","param":null,"code":null}}"#;
pub const CONTEXT_BODY: &str = r#"{"error":{"message":"This model's maximum context length is 128000 tokens. However, you requested 200000 tokens.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}}"#;
pub const NOT_FOUND_BODY: &str = r#"{"error":{"message":"The model 'no-such-model' does not exist or you do not have access to it.","type":"invalid_request_error","param":"model","code":"model_not_found"}}"#;
