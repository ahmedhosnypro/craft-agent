//! SSE reader: events parse off a byte stream, malformed bytes and
//! oversized events fail closed, and a stalled stream surfaces the idle
//! timeout instead of hanging.

use bytes::Bytes;
use craft_core::error::ProviderError;
use craft_core::sse::{SseError, SseEvent, SseOptions, SseReader};
use futures::{Stream, StreamExt};
use std::time::Duration;

fn bytes_stream(chunks: Vec<&[u8]>) -> impl Stream<Item = Result<Bytes, ProviderError>> {
    futures::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk))),
    )
}

fn opts(max_event_bytes: u64, idle_timeout: Duration) -> SseOptions {
    SseOptions {
        max_event_bytes,
        idle_timeout,
    }
}

fn event(event: &str, data: &str) -> SseEvent {
    SseEvent {
        event: String::from(event),
        data: String::from(data),
        id: String::new(),
        retry: None,
    }
}

fn transport(detail: &str) -> ProviderError {
    ProviderError::Transport {
        detail: String::from(detail),
    }
}

#[tokio::test]
async fn named_and_unnamed_events_parse() {
    let body = b"event: message\ndata: {\"a\":1}\n\ndata: {\"b\":2}\n\n";
    let mut reader = SseReader::new(
        bytes_stream(vec![body]),
        opts(1024, Duration::from_secs(30)),
    );
    assert_eq!(reader.next().await, Some(Ok(event("message", "{\"a\":1}"))));
    // Unnamed events dispatch with the SSE default type; adapters key
    // off data, not names.
    assert_eq!(reader.next().await, Some(Ok(event("message", "{\"b\":2}"))));
}

#[tokio::test]
async fn eof_ends_the_stream_cleanly() {
    let body = b"data: done\n\n";
    let mut reader = SseReader::new(
        bytes_stream(vec![body]),
        opts(1024, Duration::from_secs(30)),
    );
    assert_eq!(reader.next().await, Some(Ok(event("message", "done"))));
    assert_eq!(reader.next().await, None, "clean EOF must end the stream");
}

#[tokio::test]
async fn multi_line_data_joins_with_newlines() {
    let body = b"data: line1\ndata: line2\n\n";
    let mut reader = SseReader::new(
        bytes_stream(vec![body]),
        opts(1024, Duration::from_secs(30)),
    );
    assert_eq!(
        reader.next().await,
        Some(Ok(event("message", "line1\nline2")))
    );
}

#[tokio::test]
async fn leading_bom_is_stripped() {
    let body = "\u{feff}data: hi\n\n".as_bytes();
    let mut reader = SseReader::new(
        bytes_stream(vec![body]),
        opts(1024, Duration::from_secs(30)),
    );
    assert_eq!(reader.next().await, Some(Ok(event("message", "hi"))));
}

#[tokio::test]
async fn bom_split_across_chunks_is_stripped() {
    let chunks: Vec<&[u8]> = vec![b"\xEF", b"\xBB\xBFdata: hi\n\n"];
    let mut reader = SseReader::new(bytes_stream(chunks), opts(1024, Duration::from_secs(30)));
    assert_eq!(reader.next().await, Some(Ok(event("message", "hi"))));
}

#[tokio::test]
async fn invalid_utf8_is_a_typed_malformed_error() {
    let body: &[u8] = b"data: \xFF\xFE\n\n";
    let mut reader = SseReader::new(
        bytes_stream(vec![body]),
        opts(1024, Duration::from_secs(30)),
    );
    let next = reader.next().await;
    assert!(
        matches!(next, Some(Err(SseError::Malformed { .. }))),
        "invalid UTF-8 must be Malformed, got {next:?}"
    );
}

#[tokio::test]
async fn oversized_event_fails_closed() {
    let body = format!("data: {}\n\n", "a".repeat(200));
    let (head, tail) = body.split_at(100);
    let chunks: Vec<&[u8]> = vec![head.as_bytes(), tail.as_bytes()];
    let mut reader = SseReader::new(bytes_stream(chunks), opts(64, Duration::from_secs(30)));
    let next = reader.next().await;
    assert!(
        matches!(next, Some(Err(SseError::EventTooLarge { cap: 64 }))),
        "oversized event must be capped, got {next:?}"
    );
}

#[tokio::test]
async fn oversized_event_in_one_chunk_fails_closed() {
    let body = format!("data: {}\n\n", "a".repeat(200));
    let mut reader = SseReader::new(
        bytes_stream(vec![body.as_bytes()]),
        opts(64, Duration::from_secs(30)),
    );
    let next = reader.next().await;
    assert!(
        matches!(next, Some(Err(SseError::EventTooLarge { cap: 64 }))),
        "an event whose data exceeds the cap must be capped, got {next:?}"
    );
}

#[tokio::test]
async fn coalesced_events_in_one_chunk_pass_the_cap() {
    let body = "data: a\n\n".repeat(12);
    let mut reader = SseReader::new(
        bytes_stream(vec![body.as_bytes()]),
        opts(64, Duration::from_secs(30)),
    );
    for _ in 0..12 {
        assert_eq!(
            reader.next().await,
            Some(Ok(event("message", "a"))),
            "small events coalesced past the cap must still parse"
        );
    }
    assert_eq!(reader.next().await, None);
}

#[tokio::test]
async fn stalled_stream_surfaces_idle_timeout() {
    let head = bytes_stream(vec![b"data: partial-no-terminator\n"]);
    let stalled = head.chain(futures::stream::pending());
    let mut reader = SseReader::new(stalled, opts(1024, Duration::from_millis(150)));
    let next = reader.next().await;
    assert!(
        matches!(next, Some(Err(SseError::Idle { after })) if after == Duration::from_millis(150)),
        "stalled stream must surface Idle, got {next:?}"
    );
}

#[tokio::test]
async fn transport_errors_pass_through() {
    let stream = futures::stream::iter(vec![Err::<Bytes, _>(transport("connection reset"))]);
    let mut reader = SseReader::new(stream, opts(1024, Duration::from_secs(30)));
    let next = reader.next().await;
    assert!(
        matches!(
            next,
            Some(Err(SseError::Transport(ProviderError::Transport { .. })))
        ),
        "underlying errors must pass through, got {next:?}"
    );
}

#[test]
fn default_options_match_the_seam_baseline() {
    let options = SseOptions::default();
    assert_eq!(
        options.idle_timeout,
        Duration::from_secs(craft_core::sse::DEFAULT_IDLE_TIMEOUT_SECS)
    );
    assert_eq!(craft_core::sse::DEFAULT_IDLE_TIMEOUT_SECS, 300);
    assert_eq!(options.max_event_bytes, 1024 * 1024);
}

#[tokio::test]
async fn configured_idle_timeout_is_honored() {
    let stream = futures::stream::pending::<Result<Bytes, ProviderError>>();
    let mut reader = SseReader::new(stream, opts(1024, Duration::from_millis(80)));
    let started = std::time::Instant::now();
    let next = reader.next().await;
    assert!(
        matches!(next, Some(Err(SseError::Idle { after })) if after == Duration::from_millis(80)),
        "expected Idle, got {next:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the idle path must not hang"
    );
}

#[tokio::test]
async fn comment_lines_are_skipped() {
    let body = b": keep-alive comment\ndata: real\n\n";
    let mut reader = SseReader::new(
        bytes_stream(vec![body]),
        opts(1024, Duration::from_secs(30)),
    );
    assert_eq!(reader.next().await, Some(Ok(event("message", "real"))));
    assert_eq!(
        reader.next().await,
        None,
        "comments must not surface as events"
    );
}
