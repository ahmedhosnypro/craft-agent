//! SSE byte-stream reader for adapters: per-event size cap, per-chunk
//! idle timeout, and typed malformed-event errors.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
pub use eventsource_stream::Event as SseEvent;
use eventsource_stream::{EventStream, EventStreamError, Eventsource};
use futures::Stream;

use crate::error::ProviderError;

/// Idle-window default behind `CRAFT_AGENT_IDLE_TIMEOUT_SECS`; the same
/// seconds flow into the reader's per-chunk timeout.
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;

/// Reader limits; defaults follow the provider seam contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SseOptions {
    pub max_event_bytes: u64,
    pub idle_timeout: Duration,
}

impl Default for SseOptions {
    fn default() -> Self {
        Self {
            max_event_bytes: 1024 * 1024,
            idle_timeout: Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS),
        }
    }
}

/// Reader failures: the underlying transport error passes through and
/// everything malformed or oversized is typed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SseError<E> {
    #[error(transparent)]
    Transport(E),
    #[error("malformed SSE event: {detail}")]
    Malformed { detail: String },
    #[error("SSE event exceeded {cap} bytes")]
    EventTooLarge { cap: u64 },
    #[error("no SSE event within {after:?}")]
    Idle { after: Duration },
}
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Bytes consumed since the last yielded event, shared between the
/// byte wrapper (charges) and the reader (resets).
struct ByteBudget {
    fed: AtomicU64,
    cap: u64,
}

impl ByteBudget {
    fn charge(&self, len: usize) -> bool {
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        self.fed.fetch_add(len, Ordering::Relaxed);
        self.fed.load(Ordering::Relaxed) <= self.cap
    }

    fn reset(&self) {
        self.fed.store(0, Ordering::Relaxed);
    }
}

/// BOM strip state: a chunk shorter than the BOM itself is held back
/// until the next chunk resolves the prefix.
enum BomState {
    Pending(Vec<u8>),
    Done,
}

/// Charges forwarded bytes against the shared budget and strips a
/// leading UTF-8 BOM before the parser sees it.
struct Budgeted<S> {
    inner: Pin<Box<S>>,
    budget: Arc<ByteBudget>,
    bom: BomState,
}

impl<S> Budgeted<S> {
    fn take_bom(&mut self, bytes: Bytes) -> Option<Bytes> {
        let joined = match core::mem::replace(&mut self.bom, BomState::Done) {
            BomState::Done => bytes,
            BomState::Pending(pre) => {
                if pre.is_empty() {
                    bytes
                } else {
                    let mut all = pre;
                    all.extend_from_slice(&bytes);
                    Bytes::from(all)
                }
            }
        };
        if joined.len() < 3 {
            let pending = matches!(joined.as_ref(), [0xEF] | [0xEF, 0xBB]);
            if pending {
                self.bom = BomState::Pending(joined.to_vec());
                return None;
            }
            return Some(joined);
        }
        if joined.get(..3).is_some_and(|prefix| prefix == BOM) {
            return Some(joined.slice(3..));
        }
        Some(joined)
    }

    fn flush_pending(&mut self) -> Option<Bytes> {
        match core::mem::replace(&mut self.bom, BomState::Done) {
            BomState::Pending(pre) => Some(Bytes::from(pre)),
            BomState::Done => None,
        }
    }
}

impl<S, E> Stream for Budgeted<S>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    type Item = Result<Bytes, SseError<E>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            match this.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => match this.take_bom(bytes) {
                    Some(chunk) => {
                        if this.budget.charge(chunk.len()) {
                            return Poll::Ready(Some(Ok(chunk)));
                        }
                        return Poll::Ready(Some(Err(SseError::EventTooLarge {
                            cap: this.budget.cap,
                        })));
                    }
                    None => continue,
                },
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(Some(Err(SseError::Transport(err))));
                }
                Poll::Ready(None) => match this.flush_pending() {
                    Some(chunk) => {
                        if this.budget.charge(chunk.len()) {
                            return Poll::Ready(Some(Ok(chunk)));
                        }
                        return Poll::Ready(Some(Err(SseError::EventTooLarge {
                            cap: this.budget.cap,
                        })));
                    }
                    None => return Poll::Ready(None),
                },
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Wraps a `Result<Bytes, E>` byte stream into parsed SSE events. The
/// idle timeout wraps every poll: a connection that stops delivering
/// surfaces `SseError::Idle` instead of hanging.
pub struct SseReader<S> {
    inner: Pin<Box<EventStream<Budgeted<S>>>>,
    budget: Arc<ByteBudget>,
    options: SseOptions,
    deadline: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<S, E> SseReader<S>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    pub fn new(byte_stream: S, options: SseOptions) -> Self {
        let budget = Arc::new(ByteBudget {
            fed: AtomicU64::new(0),
            cap: options.max_event_bytes,
        });
        let inner = Budgeted {
            inner: Box::pin(byte_stream),
            budget: budget.clone(),
            bom: BomState::Pending(Vec::new()),
        };
        Self {
            inner: Box::pin(inner.eventsource()),
            budget,
            options,
            deadline: None,
        }
    }
}

fn map_stream_error<E>(err: EventStreamError<SseError<E>>) -> SseError<E> {
    match err {
        EventStreamError::Transport(e) => e,
        EventStreamError::Utf8(err) => SseError::Malformed {
            detail: format!("invalid UTF-8 in event data: {err}"),
        },
        EventStreamError::Parser(err) => SseError::Malformed {
            detail: format!("unparseable event framing: {err}"),
        },
    }
}

impl<S, E> Stream for SseReader<S>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    type Item = Result<SseEvent, SseError<E>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                this.budget.reset();
                this.deadline = None;
                if event.data.is_empty() {
                    return Poll::Ready(Some(Err(SseError::Malformed {
                        detail: String::from("event carries no data"),
                    })));
                }
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(err))) => {
                this.budget.reset();
                this.deadline = None;
                Poll::Ready(Some(Err(map_stream_error(err))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => {
                let idle = this.options.idle_timeout;
                let deadline = this
                    .deadline
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(idle)));
                match deadline.as_mut().poll(cx) {
                    Poll::Ready(()) => Poll::Ready(Some(Err(SseError::Idle { after: idle }))),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

impl From<SseError<ProviderError>> for ProviderError {
    fn from(err: SseError<ProviderError>) -> Self {
        match err {
            SseError::Transport(e) => e,
            SseError::Malformed { detail } => Self::Transport { detail },
            SseError::EventTooLarge { cap } => Self::Transport {
                detail: format!("SSE event exceeded {cap} bytes"),
            },
            SseError::Idle { .. } => Self::IdleTimeout,
        }
    }
}
