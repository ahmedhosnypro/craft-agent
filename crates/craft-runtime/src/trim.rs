//! Prompt-assembly trimming: tool results older than a turn-distance
//! window ride the assembled prompt as size stubs while the session
//! record keeps every result verbatim. Turns are transcript turns —
//! one per assistant message — so a resumed session trims its loaded
//! history by the same rule it was written under.

use craft_core::error::ProviderError;
use craft_core::types::ChatMessage;

/// The env knob bounding how many recent turns keep tool results
/// verbatim in the assembled prompt.
pub(crate) const RESULT_WINDOW_ENV: &str = "CRAFT_AGENT_RESULT_WINDOW_TURNS";

/// Turns kept verbatim when the knob is unset or empty.
pub(crate) const DEFAULT_RESULT_WINDOW_TURNS: u32 = 20;

/// One env value into the window: unset or empty keeps the default,
/// zero disables trimming, and anything unparseable is a typed error
/// naming the knob.
pub(crate) fn result_window(value: Option<&str>) -> Result<u32, ProviderError> {
    let Some(raw) = value.filter(|raw| !raw.is_empty()) else {
        return Ok(DEFAULT_RESULT_WINDOW_TURNS);
    };
    raw.trim().parse::<u32>().map_err(|_| invalid(raw))
}

/// The window from the live process environment.
pub(crate) fn result_window_from_env() -> Result<u32, ProviderError> {
    match std::env::var(RESULT_WINDOW_ENV) {
        Ok(value) => result_window(Some(&value)),
        Err(std::env::VarError::NotPresent) => result_window(None),
        Err(err) => Err(invalid(&err.to_string())),
    }
}

fn invalid(raw: &str) -> ProviderError {
    ProviderError::InvalidRequest {
        detail: format!(
            "{RESULT_WINDOW_ENV} must be a whole number of turns, 0 to disable trimming; got `{raw}`"
        ),
    }
}

/// The messages for the next request: results from the most recent
/// `window` turns ride verbatim, older ones ride a stub naming their
/// byte size and transcript turn. The session record is never
/// rewritten — trimming is prompt-assembly only.
pub(crate) fn assemble(messages: &[ChatMessage], window: u32) -> Vec<ChatMessage> {
    if window == 0 {
        return messages.to_vec();
    }
    let latest = latest_turn(messages);
    let mut turn = 0u32;
    messages
        .iter()
        .map(|message| {
            let born = turn_of(message, &mut turn);
            match message {
                ChatMessage::Tool {
                    call_id,
                    content,
                    is_error,
                    images,
                } if latest.saturating_sub(born) >= window => ChatMessage::Tool {
                    call_id: call_id.clone(),
                    content: stub(content, born),
                    is_error: *is_error,
                    images: images.clone(),
                },
                other => other.clone(),
            }
        })
        .collect()
}

/// The transcript's newest turn: assistant messages open turns, so
/// the assistant count is the turn count.
fn latest_turn(messages: &[ChatMessage]) -> u32 {
    messages
        .iter()
        .filter(|message| matches!(message, ChatMessage::Assistant { .. }))
        .count() as u32
}

/// The turn a message belongs to: an assistant message opens the next
/// turn, and everything after it rides that turn until the next opens.
fn turn_of(message: &ChatMessage, turn: &mut u32) -> u32 {
    if matches!(message, ChatMessage::Assistant { .. }) {
        *turn += 1;
    }
    *turn
}

fn stub(content: &str, turn: u32) -> String {
    format!(
        "[tool result trimmed: {} bytes, turn {turn}]",
        content.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> ChatMessage {
        ChatMessage::User {
            text: String::from(text),
            images: Vec::new(),
        }
    }

    fn assistant() -> ChatMessage {
        ChatMessage::Assistant {
            text: String::new(),
            tool_calls: Vec::new(),
            native_blocks: None,
        }
    }

    fn tool(call_id: &str, content: &str, is_error: bool) -> ChatMessage {
        ChatMessage::Tool {
            call_id: String::from(call_id),
            content: String::from(content),
            is_error,
            images: Vec::new(),
        }
    }

    // The transcript at the request for turn 4: three turns, one
    // result each.
    fn three_turns() -> Vec<ChatMessage> {
        vec![
            user("go"),
            assistant(),
            tool("c1", "0123456789", false),
            assistant(),
            tool("c2", "0123456789", false),
            assistant(),
            tool("c3", "0123456789", false),
        ]
    }

    #[test]
    fn knob_rows_follow_the_transport_parse_idiom() {
        assert_eq!(RESULT_WINDOW_ENV, "CRAFT_AGENT_RESULT_WINDOW_TURNS");
        assert_eq!(DEFAULT_RESULT_WINDOW_TURNS, 20);
        assert_eq!(result_window(None), Ok(20));
        assert_eq!(result_window(Some("")), Ok(20));
        assert_eq!(result_window(Some(" 4 ")), Ok(4));
        assert_eq!(
            result_window(Some("0")),
            Ok(0),
            "zero disables trimming, unlike the step knob's fail-fast"
        );
    }

    #[test]
    fn hostile_knob_values_surface_typed_errors_naming_the_knob() {
        for raw in ["garbage", "-5", "1.5", "1e3", "99999999999999999999"] {
            let err = result_window(Some(raw))
                .err()
                .unwrap_or_else(|| panic!("a hostile value must fail, not fall back: {raw}"));
            assert!(
                matches!(err, ProviderError::InvalidRequest { .. }),
                "got {err:?}"
            );
            assert!(err.to_string().contains(RESULT_WINDOW_ENV), "{err}");
            assert!(err.to_string().contains(raw), "{err}");
        }
    }

    #[test]
    fn zero_window_returns_the_transcript_verbatim() {
        let messages = three_turns();
        assert_eq!(assemble(&messages, 0), messages);
    }

    #[test]
    fn results_outside_the_window_ride_as_stubs_inside_it_verbatim() {
        let messages = three_turns();
        let assembled = assemble(&messages, 2);
        assert_eq!(
            assembled[2],
            tool("c1", "[tool result trimmed: 10 bytes, turn 1]", false),
            "distance 2 hits the window edge and trims"
        );
        assert_eq!(assembled[4], messages[4], "distance 1 stays verbatim");
        assert_eq!(assembled[6], messages[6], "distance 0 stays verbatim");
        assert_eq!(
            assembled[0], messages[0],
            "no tool message changes role or position"
        );
    }

    #[test]
    fn one_turn_past_the_window_keeps_everything_verbatim() {
        let messages = three_turns();
        let assembled = assemble(&messages, 3);
        assert_eq!(assembled, messages, "window 3 covers every distance");
    }

    #[test]
    fn stub_counts_bytes_not_characters() {
        let messages = vec![
            user("go"),
            assistant(),
            tool("c1", "héllo", false),
            assistant(),
        ];
        let assembled = assemble(&messages, 1);
        assert_eq!(
            assembled[2],
            tool("c1", "[tool result trimmed: 6 bytes, turn 1]", false),
            "héllo is 6 bytes and 5 characters"
        );
    }

    #[test]
    fn error_results_keep_their_flag_when_stubbed() {
        let mut messages = three_turns();
        messages[2] = tool("c1", "0123456789", true);
        let assembled = assemble(&messages, 2);
        assert!(matches!(
            &assembled[2],
            ChatMessage::Tool { is_error, content, .. } if *is_error && content.contains("trimmed")
        ));
    }

    #[test]
    fn messages_before_any_assistant_turn_are_never_trimmed() {
        let messages = vec![user("go"), tool("c0", "orphan", false)];
        assert_eq!(assemble(&messages, 1), messages);
    }
}
