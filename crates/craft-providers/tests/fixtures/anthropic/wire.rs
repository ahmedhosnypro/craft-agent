//! Recorded Anthropic Messages wire shapes: named SSE events and error
//! bodies, composed into streams the corpus serves off loopback.

/// Joins events into one SSE body: a blank line between events and a
/// terminating one after the last.
pub fn stream(events: &[&str]) -> String {
    let mut body = events.join("\n\n");
    body.push_str("\n\n");
    body
}

const EV_MESSAGE_START: &str = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"claude-haiku-5-5","content":[],"stop_reason":null,"usage":{"input_tokens":25,"output_tokens":1}}}"#;

const EV_BLOCK_START: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;

const EV_HEL: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#;

const EV_LO: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo "}}"#;

const EV_WORLD: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}"#;

const EV_BLOCK_STOP: &str = r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#;

const EV_PING: &str = "event: ping\ndata: {\"type\":\"ping\"}";

const EV_MESSAGE_DELTA: &str = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"input_tokens":25,"output_tokens":34}}"#;

const EV_MESSAGE_STOP: &str = "event: message_stop\ndata: {\"type\":\"message_stop\"}";

/// The documented lifecycle: message_start, one text block through its
/// deltas, a ping, then message_delta and message_stop.
pub fn text_stream() -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_BLOCK_START,
        EV_HEL,
        EV_LO,
        EV_WORLD,
        EV_BLOCK_STOP,
        EV_PING,
        EV_MESSAGE_DELTA,
        EV_MESSAGE_STOP,
    ])
}

const EV_TOOL_START: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_a","name":"get_weather","input":{}}}"#;

const EV_TOOL_JSON_1: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#;

const EV_TOOL_JSON_2: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"Paris\"}"}}"#;

const EV_TOOL_STOP: &str = r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#;

const EV_MESSAGE_DELTA_TOOLS: &str = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"input_tokens":40,"output_tokens":12}}"#;

/// One tool_use block assembled from input_json_delta fragments.
pub fn tool_stream() -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_TOOL_START,
        EV_TOOL_JSON_1,
        EV_TOOL_JSON_2,
        EV_TOOL_STOP,
        EV_MESSAGE_DELTA_TOOLS,
        EV_MESSAGE_STOP,
    ])
}

const EV_THINK_START: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#;

const EV_THINK_DELTA: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"weighing the request"}}"#;

const EV_SIGNATURE_DELTA: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"EqMC"}}"#;

const EV_THINK_STOP: &str = r#"event: content_block_stop
data: {"type":"content_block_stop","index":0}"#;

const EV_TEXT_START_1: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#;

const EV_HI: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"hi"}}"#;

const EV_TEXT_STOP_1: &str = r#"event: content_block_stop
data: {"type":"content_block_stop","index":1}"#;

/// A thinking block with its signature, then a text block: thinking
/// content is tolerated without surfacing as text.
pub fn thinking_stream() -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_THINK_START,
        EV_THINK_DELTA,
        EV_SIGNATURE_DELTA,
        EV_THINK_STOP,
        EV_TEXT_START_1,
        EV_HI,
        EV_TEXT_STOP_1,
        EV_MESSAGE_DELTA,
        EV_MESSAGE_STOP,
    ])
}

/// message_delta with the given stop reason, then message_stop.
pub fn finish_stream(reason: &str) -> String {
    let delta = format!(
        r#"event: message_delta
data: {{"type":"message_delta","delta":{{"stop_reason":"{reason}","stop_sequence":null}},"usage":{{"input_tokens":25,"output_tokens":6}}}}"#
    );
    stream(&[EV_MESSAGE_START, &delta, EV_MESSAGE_STOP])
}

/// message_start then a clean close: no blocks, no terminal.
pub fn unfinished_stream() -> String {
    stream(&[EV_MESSAGE_START])
}

/// The full lifecycle with no content blocks: the zero-content terminal.
pub fn zero_content_stream() -> String {
    stream(&[EV_MESSAGE_START, EV_MESSAGE_DELTA, EV_MESSAGE_STOP])
}

/// A gateway that reports no usage in any frame.
pub fn no_usage_stream() -> String {
    let start = r#"event: message_start
data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[]}}"#;
    let delta = r#"event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null}}"#;
    stream(&[start, delta, EV_MESSAGE_STOP])
}

/// The same body behind a UTF-8 BOM.
pub fn bom_text_stream() -> String {
    format!("\u{feff}{}", text_stream())
}

const EV_UNKNOWN: &str =
    "event: content_block_music\ndata: {\"type\":\"content_block_music\",\"index\":2}";

/// An event name the protocol may add later; unknown types are skipped.
pub fn unknown_event_stream() -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_BLOCK_START,
        EV_HEL,
        EV_BLOCK_STOP,
        EV_UNKNOWN,
        EV_MESSAGE_DELTA,
        EV_MESSAGE_STOP,
    ])
}

/// Properly framed SSE whose data is a truncated JSON fragment.
pub fn truncated_stream() -> String {
    String::from("event: content_block_delta\ndata: {\"type\":\"content_block_del\n\n")
}

/// Event bytes that are not valid UTF-8.
pub const INVALID_UTF8_STREAM: &[u8] =
    b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"\xFF\xFF\"}}\n\n";

/// A text delta whose index never saw a content_block_start.
pub fn orphan_delta_stream() -> String {
    stream(&[EV_MESSAGE_START, EV_HEL, EV_MESSAGE_DELTA, EV_MESSAGE_STOP])
}

/// A text delta arriving after the message_delta terminal.
pub fn late_content_stream() -> String {
    stream(&[EV_MESSAGE_START, EV_MESSAGE_DELTA, EV_HEL, EV_MESSAGE_STOP])
}

/// The lifecycle through the terminal, then the wire ends without
/// message_stop: the recorded turn stands.
pub fn wire_end_after_finish_stream() -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_BLOCK_START,
        EV_HEL,
        EV_BLOCK_STOP,
        EV_MESSAGE_DELTA,
    ])
}

/// A mid-stream error event of the given wire type.
pub fn midstream_error_stream(kind: &str) -> String {
    let event = format!(
        "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"{kind}\",\"message\":\"provider failed mid-stream\"}}}}"
    );
    stream(&[EV_MESSAGE_START, EV_BLOCK_START, EV_HEL, &event])
}

/// One text delta whose payload is oversized for the fold caps.
pub fn oversized_chunk(text: &str) -> String {
    let event = format!(
        "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"{text}\"}}}}"
    );
    event
}

/// HTTP error bodies in the documented {error:{type,message}} envelope.
pub const AUTH_BODY: &str = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key. You passed an empty or invalid key."}}"#;
pub const FORBIDDEN_BODY: &str = r#"{"type":"error","error":{"type":"permission_error","message":"Your organization does not have access to this model."}}"#;
pub const RATE_BODY: &str = r#"{"type":"error","error":{"type":"rate_limit_error","message":"Number of requests has exceeded your per-minute rate limit."}}"#;
pub const OVERLOADED_BODY: &str = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded. Try again in a few minutes."}}"#;
pub const INVALID_BODY: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: field required"}}"#;
pub const CONTEXT_BODY: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 200000 tokens > 190000 maximum context window"}}"#;
pub const SERVER_BODY: &str =
    r#"{"type":"error","error":{"type":"api_error","message":"Internal server error"}}"#;

/// message_stop with no message_delta behind it.
pub fn message_stop_only_stream() -> String {
    stream(&[EV_MESSAGE_START, EV_MESSAGE_STOP])
}

const EV_TOOL_START_Z: &str = r#"event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_z","name":"get_weather","input":{}}}"#;

/// Two tool_use blocks sharing one wire index with conflicting ids.
pub fn duplicate_tool_index_stream() -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_TOOL_START,
        EV_TOOL_START_Z,
        EV_MESSAGE_DELTA_TOOLS,
        EV_MESSAGE_STOP,
    ])
}

const EV_TOOL_JSON_BAD_1: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"Par"}}"#;

const EV_TOOL_JSON_BAD_2: &str = r#"event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"is"}}"#;

/// Argument fragments that concatenate into something that is not JSON.
pub fn bad_tool_arguments_stream() -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_TOOL_START,
        EV_TOOL_JSON_BAD_1,
        EV_TOOL_JSON_BAD_2,
        EV_TOOL_STOP,
        EV_MESSAGE_DELTA_TOOLS,
        EV_MESSAGE_STOP,
    ])
}

/// A full lifecycle whose single text delta is oversized for the fold.
pub fn oversized_text_stream(text: &str) -> String {
    stream(&[
        EV_MESSAGE_START,
        EV_BLOCK_START,
        &oversized_chunk(text),
        EV_BLOCK_STOP,
        EV_MESSAGE_DELTA,
        EV_MESSAGE_STOP,
    ])
}

/// More never-stopped content blocks than the lane keeps open.
pub fn block_flood_stream(count: usize) -> String {
    let mut body = stream(&[EV_MESSAGE_START]);
    for index in 0..count {
        let event = format!(
            "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":{index},\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n"
        );
        body.push_str(&event);
    }
    body
}
