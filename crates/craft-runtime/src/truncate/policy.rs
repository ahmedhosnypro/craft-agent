//! The per-run truncation policy: the cap pass that replaces every
//! over-cap tool result with its head plus a marker, and the spill
//! writer that parks the tail under the runner's temp mount.

use std::path::{Path, PathBuf};

use craft_core::error::ProviderError;
use craft_core::types::ChatMessage;

use super::{result_truncate_bytes_from_env, session_stem, spill_root_from_env};
use crate::runner::RunConfig;

/// One spilled tail never grows past this; the bound guards future
/// tools and hostile content, not anything a builtin can emit today.
const MAX_SPILL_FILE_BYTES: usize = 8 * 1024 * 1024;

/// Total new spill bytes one run may write.
const MAX_SPILL_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

const SPILL_DIR: &str = "craft-agent-spill";

/// The per-run truncation policy: the byte cap, the resolved spill
/// root, and the run's running spill total.
pub(crate) struct Truncate {
    cap: u64,
    root: PathBuf,
    stem: String,
    written: u64,
    max_total: u64,
}

/// Resolves the policy from the config and the environment; a hostile
/// knob fails the run here, before turn one.
pub(crate) fn resolve(config: &RunConfig) -> Result<Truncate, ProviderError> {
    let cap = match config.result_truncate_bytes {
        Some(cap) => cap,
        None => result_truncate_bytes_from_env()?,
    };
    Ok(Truncate {
        cap,
        root: spill_root_from_env(config.spill_root.clone()),
        stem: session_stem(config.session_path.as_deref()),
        written: 0,
        max_total: MAX_SPILL_TOTAL_BYTES,
    })
}

/// The messages for the next request with every over-cap result
/// reduced to its head plus a marker; trim runs first, so a stub an
/// earlier pass left is never capped a second time.
pub(crate) fn cap(messages: Vec<ChatMessage>, policy: &mut Truncate) -> Vec<ChatMessage> {
    if policy.cap == 0 {
        return messages;
    }
    messages
        .into_iter()
        .enumerate()
        .map(|(index, message)| policy.cap_message(index, message))
        .collect()
}

impl Truncate {
    /// One message through the cap: non-tool and at-cap messages ride
    /// unchanged; an over-cap result rides head plus marker.
    fn cap_message(&mut self, index: usize, message: ChatMessage) -> ChatMessage {
        match message {
            ChatMessage::Tool {
                call_id,
                content,
                is_error,
                images,
            } => {
                let Some(capped) = self.cap_content(index, &content) else {
                    return ChatMessage::Tool {
                        call_id,
                        content,
                        is_error,
                        images,
                    };
                };
                ChatMessage::Tool {
                    call_id,
                    content: capped,
                    is_error,
                    images,
                }
            }
            other => other,
        }
    }

    /// None rides the content verbatim; Some replaces it with the
    /// head plus a marker naming where the tail went.
    fn cap_content(&mut self, index: usize, content: &str) -> Option<String> {
        if content.len() as u64 <= self.cap {
            return None;
        }
        let cap = usize::try_from(self.cap).unwrap_or(usize::MAX);
        let head = craft_tools::util::truncate_char_boundary(content, cap);
        let marker = match self.spill(index, content) {
            Some(path) => with_spill_marker(head.len(), content.len(), &path),
            None => without_spill_marker(head.len(), content.len()),
        };
        Some(format!("{head}\n{marker}"))
    }

    /// The tail's spill path, or None when this tail does not spill:
    /// over the per-file bound, over the run's remaining spill
    /// budget, or storage that cannot be written. Never an error —
    /// the run outlives its scratch mount.
    fn spill(&mut self, index: usize, content: &str) -> Option<PathBuf> {
        if content.len() > MAX_SPILL_FILE_BYTES {
            return None;
        }
        if self.written.saturating_add(content.len() as u64) > self.max_total {
            return None;
        }
        let dir = self.root.join(SPILL_DIR).join(&self.stem);
        let path = dir.join(format!("result-{index:06}.txt"));
        if path.exists() {
            return Some(path);
        }
        if std::fs::create_dir_all(&dir).is_err() {
            return None;
        }
        let part = dir.join(format!("result-{index:06}.txt.part"));
        let spilled = std::fs::write(&part, content).and_then(|()| std::fs::rename(&part, &path));
        if spilled.is_err() {
            let _ = std::fs::remove_file(&part);
            return None;
        }
        self.written = self.written.saturating_add(content.len() as u64);
        Some(path)
    }
}

fn with_spill_marker(kept: usize, total: usize, path: &Path) -> String {
    let shown = path.display();
    format!(
        "[tool result truncated: kept first {kept} of {total} bytes; the full result is saved at {shown}; read it back in ranges with bash, e.g. sed -n '1,80p' {shown}]"
    )
}

fn without_spill_marker(kept: usize, total: usize) -> String {
    format!(
        "[tool result truncated: kept first {kept} of {total} bytes; the tail is not recoverable in this run — re-derive it if needed]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tool(call_id: &str, content: &str, is_error: bool) -> ChatMessage {
        ChatMessage::Tool {
            call_id: String::from(call_id),
            content: String::from(content),
            is_error,
            images: Vec::new(),
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
        let _ = std::fs::create_dir_all(&dir);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.join(format!("truncate-{tag}-{}-{n}", std::process::id()))
    }

    /// A file-shaped path: any spill write under it fails with ENOTDIR.
    fn blocked(tag: &str) -> PathBuf {
        let path = scratch(tag);
        std::fs::write(&path, b"blocked").unwrap();
        path
    }

    fn policy_at(cap: u64, root: PathBuf) -> Truncate {
        Truncate {
            cap,
            root,
            stem: String::from("stem"),
            written: 0,
            max_total: MAX_SPILL_TOTAL_BYTES,
        }
    }

    fn content_of(message: &ChatMessage) -> &str {
        let ChatMessage::Tool { content, .. } = message else {
            panic!("expected a tool message, got {message:?}");
        };
        content
    }

    fn spill_file(root: &Path) -> PathBuf {
        root.join(SPILL_DIR).join("stem").join("result-000000.txt")
    }

    #[test]
    fn at_cap_content_rides_verbatim_and_one_more_byte_truncates() {
        let mut policy = policy_at(4, scratch("atcap"));
        let at_cap = vec![tool("c1", "abcd", false)];
        assert_eq!(cap(at_cap.clone(), &mut policy), at_cap);

        let capped = cap(vec![tool("c1", "abcde", false)], &mut policy);
        let path = spill_file(&policy.root);
        assert_eq!(
            content_of(&capped[0]),
            &format!(
                "abcd\n[tool result truncated: kept first 4 of 5 bytes; the full result is saved at {}; read it back in ranges with bash, e.g. sed -n '1,80p' {}]",
                path.display(),
                path.display()
            )
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "abcde");
    }

    #[test]
    fn the_head_cut_backs_off_to_a_char_boundary() {
        let mut policy = policy_at(5, blocked("boundary"));
        let capped = cap(vec![tool("c1", "ééé", false)], &mut policy);
        assert!(
            content_of(&capped[0])
                .starts_with("éé\n[tool result truncated: kept first 4 of 6 bytes;"),
            "the head is the largest boundary prefix of the 6-byte payload"
        );
    }

    #[test]
    fn markers_count_bytes_not_characters() {
        let mut policy = policy_at(4, blocked("bytes"));
        let capped = cap(vec![tool("c1", "héllo…", false)], &mut policy);
        let content = content_of(&capped[0]);
        assert!(
            content.starts_with("hél\n"),
            "the head is h + é + l: 4 bytes"
        );
        assert!(
            content.contains("kept first 4 of 9 bytes"),
            "héllo… is 9 bytes and 6 characters: {content}"
        );
    }

    #[test]
    fn the_with_spill_marker_names_path_kept_and_total() {
        let root = scratch("markers");
        let mut policy = policy_at(4, root.clone());
        let capped = cap(vec![tool("c1", "abcde", false)], &mut policy);
        let path = spill_file(&root);
        assert_eq!(
            content_of(&capped[0]),
            &format!(
                "abcd\n[tool result truncated: kept first 4 of 5 bytes; the full result is saved at {}; read it back in ranges with bash, e.g. sed -n '1,80p' {}]",
                path.display(),
                path.display()
            )
        );

        let mut denied = policy_at(4, blocked("markers"));
        let capped = cap(vec![tool("c1", "abcde", false)], &mut denied);
        assert_eq!(
            content_of(&capped[0]),
            "abcd\n[tool result truncated: kept first 4 of 5 bytes; the tail is not recoverable in this run — re-derive it if needed]"
        );
    }

    #[test]
    fn spill_write_failure_downgrades_to_the_no_spill_marker() {
        let mut policy = policy_at(4, blocked("downgrade").join("spill"));
        let capped = cap(vec![tool("c1", "abcde", false)], &mut policy);
        let content = content_of(&capped[0]);
        assert!(content.starts_with("abcd\n[tool result truncated: kept first 4 of 5 bytes;"));
        assert!(content.contains("not recoverable"), "{content}");
        assert!(!content.contains("saved at"), "{content}");
    }

    #[test]
    fn spill_writes_once_and_is_reused() {
        let root = scratch("reuse");
        let mut policy = policy_at(4, root.clone());
        cap(vec![tool("c1", "abcde", false)], &mut policy);
        let path = spill_file(&root);
        std::fs::write(&path, "SENTINEL").unwrap();

        let again = cap(vec![tool("c1", "abcde", false)], &mut policy);
        assert!(
            content_of(&again[0]).contains("saved at"),
            "the existing file is reused as the pointer"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "SENTINEL",
            "an existing spill file is never rewritten"
        );
    }

    #[test]
    fn the_aggregate_ceiling_stops_spilling_but_never_capping() {
        let root = scratch("ceiling");
        let mut policy = policy_at(4, root.clone());
        policy.max_total = 5;
        let messages = vec![tool("c1", "abcde", false), tool("c2", "fghij", false)];
        let capped = cap(messages, &mut policy);
        assert!(
            content_of(&capped[0]).contains("saved at"),
            "the first tail fits the budget"
        );
        assert!(
            content_of(&capped[1]).contains("not recoverable"),
            "the second tail exceeds the budget and caps without a pointer"
        );
        assert_eq!(
            std::fs::read_to_string(spill_file(&root)).unwrap(),
            "abcde",
            "earlier files stand"
        );
    }

    #[test]
    fn results_over_the_per_file_guard_never_spill() {
        let root = scratch("perfile");
        let mut policy = policy_at(4, root.clone());
        let huge = "x".repeat(MAX_SPILL_FILE_BYTES + 1);
        let capped = cap(vec![tool("c1", &huge, false)], &mut policy);
        assert!(
            content_of(&capped[0]).contains("not recoverable"),
            "no pointer for a tail over the per-file guard"
        );
        assert!(
            !root.join(SPILL_DIR).exists(),
            "the guard fires before any write"
        );
    }

    #[test]
    fn error_results_keep_their_flag_when_truncated() {
        let mut policy = policy_at(4, blocked("flag"));
        let capped = cap(vec![tool("c1", "abcde", true)], &mut policy);
        assert!(matches!(
            &capped[0],
            ChatMessage::Tool { is_error, content, .. }
            if *is_error && content.starts_with("abcd\n[tool result truncated: kept first 4 of 5 bytes;")
        ));
    }
}
