//! The append-only NDJSON journal: dense seq, torn-tail repair, hostile-file
//! refusal, and caps enforced at both load and record.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::WorkflowError;
use crate::plan::Limits;
use crate::status::{RunConclusion, SkipReason, StepConclusion, StepUsage};

pub const JOURNAL_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const JOURNAL_MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JournalEvent {
    RunStarted {
        seq: u64,
        run_id: String,
        name: String,
        plan_hash: String,
        args_digest: String,
        limits: Limits,
    },
    StepStarted {
        seq: u64,
        id: String,
    },
    StepFinished {
        seq: u64,
        id: String,
        input_hash: String,
        conclusion: StepConclusion,
        usage: StepUsage,
        final_message: String,
    },
    StepSkipped {
        seq: u64,
        id: String,
        reason: SkipReason,
    },
    RunFinished {
        seq: u64,
        conclusion: RunConclusion,
        failed_step: Option<String>,
        total_usage: StepUsage,
    },
}

impl JournalEvent {
    pub fn seq(&self) -> u64 {
        match self {
            Self::RunStarted { seq, .. }
            | Self::StepStarted { seq, .. }
            | Self::StepFinished { seq, .. }
            | Self::StepSkipped { seq, .. }
            | Self::RunFinished { seq, .. } => *seq,
        }
    }
}

#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    entries: Vec<JournalEvent>,
    bytes: u64,
    max_bytes: u64,
    max_entries: usize,
}

impl Journal {
    pub fn open(path: &Path) -> Result<Self, WorkflowError> {
        Self::open_bounded(path, JOURNAL_MAX_BYTES, JOURNAL_MAX_ENTRIES)
    }

    /// Load-or-fresh with explicit caps; every failure is typed.
    pub fn open_bounded(
        path: &Path,
        max_bytes: u64,
        max_entries: usize,
    ) -> Result<Self, WorkflowError> {
        let shown = path.to_string_lossy().into_owned();
        let meta = match std::fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path: path.to_path_buf(),
                    entries: Vec::new(),
                    bytes: 0,
                    max_bytes,
                    max_entries,
                });
            }
            Err(err) => {
                return Err(WorkflowError::JournalIo {
                    path: shown,
                    detail: err.to_string(),
                });
            }
        };
        if meta.file_type().is_symlink() {
            return Err(WorkflowError::JournalUnsafe {
                path: shown,
                reason: "is a symlink".to_string(),
            });
        }
        if !meta.is_file() {
            return Err(WorkflowError::JournalUnsafe {
                path: shown,
                reason: "is not a regular file".to_string(),
            });
        }
        if meta.len() > max_bytes {
            return Err(WorkflowError::JournalCap {
                path: shown,
                limit: max_bytes,
            });
        }
        let mut text = String::new();
        let handle = std::fs::File::open(path).map_err(|err| WorkflowError::JournalIo {
            path: shown.clone(),
            detail: err.to_string(),
        })?;
        handle
            .take(max_bytes + 1)
            .read_to_string(&mut text)
            .map_err(|err| WorkflowError::JournalIo {
                path: shown.clone(),
                detail: err.to_string(),
            })?;
        if text.len() as u64 > max_bytes {
            return Err(WorkflowError::JournalCap {
                path: shown,
                limit: max_bytes,
            });
        }
        Self::scan(path, text, max_bytes, max_entries)
    }
}

impl Journal {
    fn scan(
        path: &Path,
        text: String,
        max_bytes: u64,
        max_entries: usize,
    ) -> Result<Self, WorkflowError> {
        let shown = path.to_string_lossy().into_owned();
        let mut entries: Vec<JournalEvent> = Vec::new();
        let bytes = text.as_bytes();
        let mut pos = 0usize;
        let mut line_no: u64 = 0;
        let mut out_bytes = 0u64;
        loop {
            let Some(rel) = bytes[pos..].iter().position(|&b| b == b'\n') else {
                let chunk = &text[pos..];
                if !chunk.trim().is_empty() {
                    let parsed = serde_json::from_str::<JournalEvent>(chunk.trim());
                    match parsed {
                        Ok(event) if event.seq() == entries.len() as u64 => {
                            if entries.len() >= max_entries {
                                return Err(WorkflowError::JournalCap {
                                    path: shown,
                                    limit: max_entries as u64,
                                });
                            }
                            Self::append_bytes(path, &shown, b"\n")?;
                            out_bytes = (pos + 1) as u64;
                            entries.push(event);
                        }
                        _ => {
                            Self::truncate_to(path, &shown, pos as u64)?;
                        }
                    }
                } else if !chunk.is_empty() {
                    Self::truncate_to(path, &shown, pos as u64)?;
                }
                break;
            };
            let end = pos + rel;
            line_no += 1;
            let event: JournalEvent =
                serde_json::from_str(text[pos..end].trim()).map_err(|err| {
                    WorkflowError::JournalParse {
                        path: shown.clone(),
                        line: line_no,
                        detail: err.to_string(),
                    }
                })?;
            if event.seq() != entries.len() as u64 {
                return Err(WorkflowError::JournalSequence {
                    path: shown,
                    expected: entries.len() as u64,
                    actual: event.seq(),
                });
            }
            if entries.len() >= max_entries {
                return Err(WorkflowError::JournalCap {
                    path: shown,
                    limit: max_entries as u64,
                });
            }
            entries.push(event);
            pos = end + 1;
            out_bytes = pos as u64;
        }
        Ok(Self {
            path: path.to_path_buf(),
            entries,
            bytes: out_bytes,
            max_bytes,
            max_entries,
        })
    }

    fn append_bytes(path: &Path, shown: &str, tail: &[u8]) -> Result<(), WorkflowError> {
        let mut handle = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .map_err(|err| WorkflowError::JournalIo {
                path: shown.to_string(),
                detail: err.to_string(),
            })?;
        handle
            .write_all(tail)
            .map_err(|err| WorkflowError::JournalIo {
                path: shown.to_string(),
                detail: err.to_string(),
            })?;
        Ok(())
    }

    /// Torn-tail repair: keep the valid prefix, drop the trailing damage.
    fn truncate_to(path: &Path, shown: &str, offset: u64) -> Result<(), WorkflowError> {
        let handle =
            OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|err| WorkflowError::JournalIo {
                    path: shown.to_string(),
                    detail: err.to_string(),
                })?;
        handle
            .set_len(offset)
            .map_err(|err| WorkflowError::JournalIo {
                path: shown.to_string(),
                detail: err.to_string(),
            })?;
        Ok(())
    }

    pub fn next_seq(&self) -> u64 {
        self.entries.len() as u64
    }

    pub fn record(&mut self, event: JournalEvent) -> Result<u64, WorkflowError> {
        let shown = self.path.to_string_lossy().into_owned();
        let expected = self.entries.len() as u64;
        if event.seq() != expected {
            return Err(WorkflowError::JournalSequence {
                path: shown,
                expected,
                actual: event.seq(),
            });
        }
        if self.entries.len() >= self.max_entries {
            return Err(WorkflowError::JournalFull {
                path: shown,
                seq: expected,
                limit: self.max_entries,
            });
        }
        let mut line = serde_json::to_string(&event).map_err(|err| WorkflowError::JournalIo {
            path: shown.clone(),
            detail: err.to_string(),
        })?;
        line.push('\n');
        if self.bytes + line.len() as u64 > self.max_bytes {
            return Err(WorkflowError::JournalCap {
                path: shown,
                limit: self.max_bytes,
            });
        }
        Self::append_bytes(&self.path, &shown, line.as_bytes())?;
        let seq = event.seq();
        self.entries.push(event);
        self.bytes += line.len() as u64;
        Ok(seq)
    }
}

impl Journal {
    pub fn entries(&self) -> &[JournalEvent] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn path_str(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}
