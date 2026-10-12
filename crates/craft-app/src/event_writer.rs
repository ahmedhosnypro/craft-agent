//! Run-event emission: a bounded queue and a dedicated writer thread so
//! a dead or stalled stdout consumer never stalls the runtime thread,
//! plus the driver that ends the run with a typed outcome the moment
//! the consumer goes away.

use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::Duration;

use tokio::sync::{oneshot, watch};

use crate::cli::OutputFormat;
use crate::error::AppError;
use crate::stream_json::{write_event, write_plain_event};
use crate::{CancelCategory, ExitStatus, RunOutcome, RuntimeEvent, Usage};

/// Bound on queued events: a consumer that stops reading loses events
/// instead of stalling the run.
pub const EVENT_QUEUE_CAP: usize = 64;

/// Health flag shared by the sink and the writer: `true` once stdout can
/// no longer take events.
pub type ClosedSignal = watch::Sender<bool>;
pub type ClosedWatch = watch::Receiver<bool>;

/// Detail for the consumer-closed abort; also the stderr note.
const CLOSED_DETAIL: &str =
    "run aborted: the run-event stream consumer closed before the run finished";
/// Window the writer gets to drain its queue once the run has ended.
const DRAIN_WINDOW: Duration = Duration::from_secs(10);

/// The write side the agent loop holds; emit never blocks.
pub struct EventSink {
    tx: SyncSender<RuntimeEvent>,
    closed: ClosedSignal,
}

impl EventSink {
    /// A full queue drops the event; a closed queue flags the run.
    pub fn emit(&self, event: RuntimeEvent) {
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {
                let _ = self.closed.send(true);
            }
        }
    }
}

/// The driver's half: the closed watch to select on and the writer's
/// done signal for a bounded final drain.
pub struct EventWriterHandle {
    closed: ClosedWatch,
    done: oneshot::Receiver<()>,
}

impl EventWriterHandle {
    pub fn closed(&self) -> &ClosedWatch {
        &self.closed
    }

    /// Bounded drain: a healthy consumer reads the tail; a stalled one
    /// is left behind and the process exits anyway.
    pub async fn drain(self) {
        if tokio::time::timeout(DRAIN_WINDOW, self.done).await.is_err() {
            eprintln!(
                "craft-agent: run-event writer did not drain within {DRAIN_WINDOW:?}; leaving it behind"
            );
        }
    }
}

struct QueueParts {
    sink: EventSink,
    handle: EventWriterHandle,
    events: Receiver<RuntimeEvent>,
    done: oneshot::Sender<()>,
}

fn open_event_queue() -> QueueParts {
    let (tx, events) = mpsc::sync_channel(EVENT_QUEUE_CAP);
    let (closed, closed_rx) = watch::channel(false);
    let (done_tx, done_rx) = oneshot::channel();
    QueueParts {
        sink: EventSink {
            tx,
            closed: closed.clone(),
        },
        handle: EventWriterHandle {
            closed: closed_rx,
            done: done_rx,
        },
        events,
        done: done_tx,
    }
}

/// Sinks the queue into stdout on a dedicated writer thread. The join
/// handle is dropped on purpose: the done signal and process exit bound
/// the thread's life.
pub fn spawn_event_writer(
    format: OutputFormat,
) -> Result<(EventSink, EventWriterHandle), AppError> {
    let QueueParts {
        sink,
        handle,
        events,
        done,
    } = open_event_queue();
    let writer_closed = sink.closed.clone();
    std::thread::Builder::new()
        .name(String::from("run-event-writer"))
        .spawn(move || {
            write_events(
                std::io::stdout().lock(),
                format,
                events,
                writer_closed,
                done,
            );
        })
        .map_err(|source| AppError::EventWriter { source })?;
    Ok((sink, handle))
}

/// Drains the queue into `out` until the queue closes; the first write
/// failure flags the consumer and stops the loop, so a dead consumer
/// costs one stderr line, not one per event.
fn write_events<W: Write>(
    mut out: W,
    format: OutputFormat,
    events: Receiver<RuntimeEvent>,
    closed: ClosedSignal,
    done: oneshot::Sender<()>,
) {
    while let Ok(event) = events.recv() {
        let result = match format {
            OutputFormat::StreamJson => write_event(&mut out, &event),
            OutputFormat::Plain => write_plain_event(&mut out, &event),
        };
        if let Err(error) = result {
            eprintln!("craft-agent: run-event write failed: {error}");
            let _ = closed.send(true);
            break;
        }
    }
    let _ = done.send(());
}

fn zero_usage() -> Usage {
    Usage {
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        incomplete: false,
    }
}

/// Drives the run future to its outcome, ending the run with a typed
/// error outcome the moment the consumer goes away, then gives the
/// writer a bounded window to drain what is left.
pub async fn drive(
    run: impl Future<Output = RunOutcome>,
    writer: EventWriterHandle,
    session_path: Option<PathBuf>,
) -> RunOutcome {
    let mut closed = writer.closed().clone();
    let _ = closed.borrow_and_update();
    let outcome = tokio::select! {
        outcome = run => outcome,
        _ = closed.changed() => {
            eprintln!("craft-agent: {CLOSED_DETAIL}");
            RunOutcome {
                exit: ExitStatus::Error,
                category: CancelCategory::Error,
                detail: CLOSED_DETAIL.to_string(),
                usage: zero_usage(),
                turns: 0,
                session_path,
            }
        }
    };
    writer.drain().await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("pipe closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("pipe closed"))
        }
    }

    #[test]
    fn write_failure_flags_the_consumer_and_stops_the_loop() {
        let (tx, events) = mpsc::sync_channel(EVENT_QUEUE_CAP);
        let (closed, closed_rx) = watch::channel(false);
        let (done_tx, mut done_rx) = oneshot::channel();
        for turn in 1..=3 {
            tx.send(RuntimeEvent::TurnStarted { turn })
                .expect("event queued");
        }
        drop(tx);
        write_events(Broken, OutputFormat::StreamJson, events, closed, done_tx);
        assert!(
            *closed_rx.borrow(),
            "first write failure must flag the consumer"
        );
        done_rx.try_recv().expect("writer reports exit");
    }

    #[test]
    fn healthy_writer_drains_every_queued_event() {
        let (tx, events) = mpsc::sync_channel(EVENT_QUEUE_CAP);
        let (closed, closed_rx) = watch::channel(false);
        let (done_tx, mut done_rx) = oneshot::channel();
        for turn in 1..=3 {
            tx.send(RuntimeEvent::TurnStarted { turn })
                .expect("event queued");
        }
        drop(tx);
        let mut buf = Vec::new();
        write_events(&mut buf, OutputFormat::StreamJson, events, closed, done_tx);
        let text = String::from_utf8(buf).expect("valid utf-8");
        assert_eq!(text.lines().count(), 3);
        assert!(!*closed_rx.borrow());
        done_rx.try_recv().expect("writer reports exit");
    }

    #[test]
    fn full_queue_drops_events_without_aborting() {
        let (tx, _events) = mpsc::sync_channel(2);
        let (closed, closed_rx) = watch::channel(false);
        let sink = EventSink { tx, closed };
        for turn in 1..=5 {
            sink.emit(RuntimeEvent::TurnStarted { turn });
        }
        assert!(
            !*closed_rx.borrow(),
            "a stalled consumer must not abort the run"
        );
    }

    #[test]
    fn disconnected_queue_flags_the_run_for_abort() {
        let (tx, events) = mpsc::sync_channel(2);
        let (closed, closed_rx) = watch::channel(false);
        let sink = EventSink { tx, closed };
        drop(events);
        sink.emit(RuntimeEvent::TurnStarted { turn: 1 });
        assert!(*closed_rx.borrow(), "a closed consumer must flag the run");
    }

    #[tokio::test]
    async fn consumer_closed_mid_run_ends_with_typed_error_outcome() {
        let QueueParts {
            sink,
            handle,
            events: _,
            done,
        } = open_event_queue();
        let _ = done.send(());
        let task = tokio::spawn(drive(std::future::pending::<RunOutcome>(), handle, None));
        tokio::task::yield_now().await;
        let _ = sink.closed.send(true);
        let outcome = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("drive returns")
            .expect("drive task joins");
        assert_eq!(outcome.exit_code(), 1);
        assert_eq!(outcome.category, CancelCategory::Error);
        assert_eq!(outcome.detail, CLOSED_DETAIL);
    }

    #[tokio::test]
    async fn healthy_consumer_passes_the_run_outcome_through() {
        let QueueParts {
            sink: _sink,
            handle,
            events: _,
            done,
        } = open_event_queue();
        let _ = done.send(());
        let run = std::future::ready(RunOutcome {
            exit: ExitStatus::Ok,
            category: CancelCategory::Completed,
            detail: "done".to_string(),
            usage: zero_usage(),
            turns: 1,
            session_path: None,
        });
        let outcome = drive(run, handle, None).await;
        assert_eq!(outcome.exit_code(), 0);
        assert_eq!(outcome.detail, "done");
    }
}
