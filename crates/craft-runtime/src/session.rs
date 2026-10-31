//! Append-only session records for a run: one JSON `ChatMessage` per
//! line, loaded verbatim on open so a resumed run replays the exact
//! transcript the model saw.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use craft_core::types::ChatMessage;

const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Writes at or above this size ride a writer thread; smaller writes
/// are faster inline than a thread round-trip.
const IO_OFFLOAD_MIN_BYTES: usize = 256 * 1024;

/// Runs one bounded write: payloads under the offload bound write
/// inline; larger ones ride a short-lived writer thread that signals
/// completion over a oneshot, so the executor keeps driving in-flight
/// work. A spawn failure or a dead thread is an error, never a panic.
pub(crate) async fn write_bounded(
    bytes: usize,
    work: impl FnOnce() -> std::io::Result<()> + Send + 'static,
) -> std::io::Result<()> {
    if bytes < IO_OFFLOAD_MIN_BYTES {
        return work();
    }
    let (done, wait) = futures::channel::oneshot::channel();
    match std::thread::Builder::new().spawn(move || {
        let _ = done.send(work());
    }) {
        Ok(_) => wait
            .await
            .unwrap_or_else(|_| std::io::Result::Err(std::io::Error::other("writer thread ended"))),
        Err(err) => Err(err),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session open failed: {msg}")]
    Open { msg: String },
    #[error("session read failed: {msg}")]
    Read { msg: String },
    #[error("session write failed: {msg}")]
    Write { msg: String },
    #[error("session line {line} is not a valid message record: {detail}")]
    CorruptLine { line: usize, detail: String },
    #[error("session line {line} exceeds the {cap}-byte cap")]
    LineTooLarge { line: usize, cap: usize },
    #[error("session file exceeds the {cap}-byte cap")]
    FileTooLarge { cap: u64 },
}

/// The run transcript: every model-visible message, appended once and
/// never rewritten. Entries are shared, so request assembly bumps
/// pointers instead of copying records. `None` keeps the run in memory
/// only.
pub struct Session {
    file: Option<File>,
    messages: Vec<std::sync::Arc<ChatMessage>>,
}

impl Session {
    pub fn in_memory() -> Self {
        Self {
            file: None,
            messages: Vec::new(),
        }
    }

    /// Opens (creating if needed) the append-only record, loading any
    /// existing lines first; a corrupt or oversized file is a typed
    /// error, never a panic.
    pub fn open(path: PathBuf) -> Result<Self, SessionError> {
        let messages = if path.exists() {
            Self::load(&path)?
                .into_iter()
                .map(std::sync::Arc::new)
                .collect()
        } else {
            Vec::new()
        };
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|err| SessionError::Open {
                msg: err.to_string(),
            })?;
        Ok(Self {
            file: Some(file),
            messages,
        })
    }

    /// Parses an existing record file into messages, in order; every
    /// record must end in a newline so an append can never fuse records.
    pub fn load(path: &Path) -> Result<Vec<ChatMessage>, SessionError> {
        let file = File::open(path).map_err(|err| SessionError::Read {
            msg: err.to_string(),
        })?;
        if file
            .metadata()
            .map_err(|err| SessionError::Read {
                msg: err.to_string(),
            })?
            .len()
            > MAX_FILE_BYTES
        {
            return Err(SessionError::FileTooLarge {
                cap: MAX_FILE_BYTES,
            });
        }
        let mut reader = BufReader::new(file);
        let mut raw: Vec<u8> = Vec::new();
        let mut messages = Vec::new();
        let mut index = 0usize;
        loop {
            raw.clear();
            let read = reader
                .read_until(b'\n', &mut raw)
                .map_err(|err| SessionError::Read {
                    msg: err.to_string(),
                })?;
            if read == 0 {
                break;
            }
            index += 1;
            if raw.last() != Some(&b'\n') {
                return Err(SessionError::CorruptLine {
                    line: index,
                    detail: String::from("record is not newline-terminated"),
                });
            }
            raw.pop();
            if raw.last() == Some(&b'\r') {
                raw.pop();
            }
            if raw.len() > MAX_LINE_BYTES {
                return Err(SessionError::LineTooLarge {
                    line: index,
                    cap: MAX_LINE_BYTES,
                });
            }
            if raw.is_empty() {
                return Err(SessionError::CorruptLine {
                    line: index,
                    detail: String::from("blank line"),
                });
            }
            let line = std::str::from_utf8(&raw).map_err(|err| SessionError::Read {
                msg: err.to_string(),
            })?;
            let message = serde_json::from_str(line).map_err(|err| SessionError::CorruptLine {
                line: index,
                detail: err.to_string(),
            })?;
            messages.push(message);
        }
        Ok(messages)
    }

    /// Appends one model-visible message to the transcript and the
    /// record; never rewrites what is already there. Large lines ride
    /// the bounded writer thread so the executor keeps driving.
    pub async fn append(&mut self, message: ChatMessage) -> Result<(), SessionError> {
        if let Some(file) = &mut self.file {
            let mut line = serde_json::to_string(&message).map_err(|err| SessionError::Write {
                msg: err.to_string(),
            })?;
            line.push('\n');
            let mut writer = file.try_clone().map_err(|err| SessionError::Write {
                msg: err.to_string(),
            })?;
            write_bounded(line.len(), move || writer.write_all(line.as_bytes()))
                .await
                .map_err(|err| SessionError::Write {
                    msg: err.to_string(),
                })?;
        }
        self.messages.push(std::sync::Arc::new(message));
        Ok(())
    }

    /// The messages the model sees on the next request, in order;
    /// entries are shared with every past and future assembled request.
    pub fn messages(&self) -> &[std::sync::Arc<ChatMessage>] {
        &self.messages
    }

    /// Replaces the in-memory history (auto-compaction swaps the prefix
    /// for one summary message); the append-only record on disk is
    /// never rewritten.
    pub(crate) fn replace_messages(&mut self, messages: Vec<std::sync::Arc<ChatMessage>>) {
        self.messages = messages;
    }
}
