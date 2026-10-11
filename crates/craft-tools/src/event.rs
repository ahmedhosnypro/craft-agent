//! Tool events and the stream contract: any number of Progress events,
//! then exactly one terminal item (a Terminal event or a typed error).

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::stream::{self, Stream, StreamExt};

use crate::error::ToolError;

/// Which child stream a progress chunk came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutStream {
    Stdout,
    Stderr,
}

/// Partial tool output, surfaced while the tool is still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProgress {
    pub stream: OutStream,
    pub text: String,
}

impl ToolProgress {
    pub fn stdout(text: String) -> Self {
        Self {
            stream: OutStream::Stdout,
            text,
        }
    }

    pub fn stderr(text: String) -> Self {
        Self {
            stream: OutStream::Stderr,
            text,
        }
    }
}

/// The single terminal result of a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolEvent {
    Progress(ToolProgress),
    Terminal(ToolResult),
}

pub type ToolStream = Pin<Box<dyn Stream<Item = Result<ToolEvent, ToolError>> + Send>>;

/// A stream that yields one typed error and then ends.
pub fn error_stream(err: ToolError) -> ToolStream {
    stream::iter(vec![Err(err)]).boxed()
}

/// A stream that yields one terminal result and ends.
pub fn result_stream(output: String) -> ToolStream {
    stream::iter(vec![Ok(ToolEvent::Terminal(ToolResult { output }))]).boxed()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Awaiting,
    Done,
    Violated,
}

/// Enforces the progress-then-terminal contract on an arbitrary tool
/// stream: exactly one terminal item, nothing after it, and a typed
/// error for a terminal-less end.
struct Contracted {
    inner: ToolStream,
    phase: Phase,
}

impl Stream for Contracted {
    type Item = Result<ToolEvent, ToolError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.phase == Phase::Violated {
            return Poll::Ready(None);
        }
        match this.inner.as_mut().poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                if this.phase == Phase::Awaiting {
                    this.phase = Phase::Done;
                    Poll::Ready(Some(Err(ToolError::MissingTerminal)))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Ready(Some(Ok(ToolEvent::Progress(p)))) => match this.phase {
                Phase::Awaiting => Poll::Ready(Some(Ok(ToolEvent::Progress(p)))),
                _ => {
                    this.phase = Phase::Violated;
                    Poll::Ready(Some(Err(ToolError::TrailingProgress)))
                }
            },
            Poll::Ready(Some(Ok(ToolEvent::Terminal(t)))) => match this.phase {
                Phase::Awaiting => {
                    this.phase = Phase::Done;
                    Poll::Ready(Some(Ok(ToolEvent::Terminal(t))))
                }
                _ => {
                    this.phase = Phase::Violated;
                    Poll::Ready(Some(Err(ToolError::DoubleTerminal)))
                }
            },
            Poll::Ready(Some(Err(e))) => match this.phase {
                Phase::Awaiting => {
                    this.phase = Phase::Done;
                    Poll::Ready(Some(Err(e)))
                }
                _ => {
                    this.phase = Phase::Violated;
                    Poll::Ready(Some(Err(ToolError::DoubleTerminal)))
                }
            },
        }
    }
}

/// Wrap a raw tool stream with the contract enforcer.
pub fn enforce_contract(inner: ToolStream) -> ToolStream {
    Contracted {
        inner,
        phase: Phase::Awaiting,
    }
    .boxed()
}
