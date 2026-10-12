//! RED corpus: agent-file value shapes and body bounds — numeric and
//! boolean fields, the prompt-mode enum, body presence and size, the
//! file-size cap, and the full-field round trip.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use craft_core::agents::{PromptMode, load_file};

fn scratch(name: &str) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "craft-agents-b-{}-{name}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write(dir: &Path, file: &str, text: &str) -> std::io::Result<PathBuf> {
    let path = dir.join(file);
    fs::write(&path, text)?;
    Ok(path)
}

/// A file whose frontmatter carries `extra` after a valid identity.
fn agent(file: &str, extra: &str) -> std::io::Result<PathBuf> {
    let dir = scratch(file)?;
    write(
        &dir,
        file,
        &format!("---\nname: probe\ndescription: Use for probing.\n{extra}---\nBody.\n"),
    )
}

fn expect(path: &Path, fragment: &str) {
    let text = match load_file(path) {
        Err(error) => error.to_string(),
        Ok(_) => String::from("the file was accepted"),
    };
    assert!(text.contains(fragment), "expected `{fragment}` in `{text}`");
}

#[test]
fn zero_max_turns_is_rejected() {
    let path = agent("probe.md", "maxTurns: 0\n").unwrap();
    expect(&path, "maxTurns must be a whole number of at least 1");
}

#[test]
fn nonnumeric_max_turns_is_rejected() {
    let path = agent("probe.md", "maxTurns: many\n").unwrap();
    expect(&path, "maxTurns must be a whole number of at least 1");
}

#[test]
fn zero_timeout_is_rejected() {
    let path = agent("probe.md", "timeoutSecs: 0\n").unwrap();
    expect(&path, "timeoutSecs must be a whole number of at least 1");
}

#[test]
fn bad_prompt_mode_is_rejected() {
    let path = agent("probe.md", "promptMode: replace\n").unwrap();
    expect(&path, "promptMode must be extend or full");
}

#[test]
fn bad_bool_is_rejected() {
    let path = agent("probe.md", "background: maybe\n").unwrap();
    expect(&path, "background must be true or false");
}

#[test]
fn bad_agents_md_is_rejected() {
    let path = agent("probe.md", "agentsMd: yes\n").unwrap();
    expect(&path, "agentsMd must be true or false");
}

#[test]
fn empty_body_is_rejected() {
    let dir = scratch("nobody").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: probe\ndescription: Use for probing.\n---\n",
    )
    .unwrap();
    expect(&path, "no prompt body after the frontmatter");
}

#[test]
fn whitespace_body_is_rejected() {
    let dir = scratch("blankbody").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: probe\ndescription: Use for probing.\n---\n \n \n",
    )
    .unwrap();
    expect(&path, "no prompt body after the frontmatter");
}

#[test]
fn oversized_body_is_rejected() {
    let dir = scratch("longbody").unwrap();
    let body: String = "line\n".repeat(301);
    let path = write(
        &dir,
        "probe.md",
        &format!("---\nname: probe\ndescription: Use for probing.\n---\n{body}"),
    )
    .unwrap();
    expect(&path, "301 effective body lines (cap 300)");
}

#[test]
fn oversized_file_is_rejected() {
    let dir = scratch("bigfile").unwrap();
    let body = format!("{}\n", "x".repeat(70_000));
    let path = write(
        &dir,
        "probe.md",
        &format!("---\nname: probe\ndescription: Use for probing.\n---\n{body}"),
    )
    .unwrap();
    expect(&path, "file is 65");
    expect(&path, "bytes (cap 65536)");
}

#[test]
fn comment_and_blank_body_lines_do_not_count_against_the_cap() {
    let dir = scratch("bodycount").unwrap();
    let mut body = String::new();
    for _ in 0..300 {
        body.push_str("line\n");
    }
    for extra in ["\n", "// comment\n", "/* block\n", "still block */\n"] {
        body.push_str(extra);
    }
    let path = write(
        &dir,
        "probe.md",
        &format!("---\nname: probe\ndescription: Use for probing.\n---\n{body}"),
    )
    .unwrap();
    load_file(&path).expect("comments and blanks must not count as body lines");
}

#[test]
fn round_trip_carries_every_field() {
    let dir = scratch("roundtrip").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: probe\ndescription: Use for probing.\nmodel: kimi-k3\npromptMode: full\n\
         tools: bash, read_file\ndisallowedTools: gh\nmaxTurns: 15\ntimeoutSecs: 600\n\
         background: true\nagentsMd: false\n---\nBody text.\n",
    )
    .unwrap();
    let agent = load_file(&path).expect("a full profile must load");
    assert_eq!(agent.name, "probe");
    assert_eq!(agent.description, "Use for probing.");
    assert_eq!(agent.model, Some("kimi-k3".to_string()));
    assert_eq!(agent.prompt_mode, PromptMode::Full);
    assert_eq!(agent.tools, Some(vec!["bash".into(), "read_file".into()]));
    assert_eq!(agent.disallowed_tools, vec!["gh".to_string()]);
    assert_eq!(agent.max_turns, Some(15));
    assert_eq!(agent.timeout_secs, Some(600));
    assert!(agent.background);
    assert!(!agent.agents_md);
    assert_eq!(agent.body, "Body text.");
    assert_eq!(agent.path, path);
}

#[test]
fn model_inherit_normalizes_to_none() {
    let path = agent("probe.md", "model: inherit\n").unwrap();
    let agent = load_file(&path).expect("model inherit must load");
    assert_eq!(agent.model, None);
}
