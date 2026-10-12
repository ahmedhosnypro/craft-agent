//! RED corpus: agent-file frontmatter parsing — fences, structural
//! findings, and value folding, mirrored from the agent-file gate.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use craft_core::agents::{PromptMode, load_file};

fn scratch(name: &str) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "craft-agents-{}-{name}-{unique}",
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

/// A minimal valid profile file; `frontmatter` carries full lines.
fn agent(file: &str, frontmatter: &str, body: &str) -> std::io::Result<PathBuf> {
    let dir = scratch(file)?;
    write(&dir, file, &format!("---\n{frontmatter}---\n{body}"))
}

fn expect(path: &Path, fragment: &str) {
    let text = match load_file(path) {
        Err(error) => error.to_string(),
        Ok(_) => String::from("the file was accepted"),
    };
    assert!(text.contains(fragment), "expected `{fragment}` in `{text}`");
}

#[test]
fn missing_leading_fence_is_rejected() {
    let path = write(
        &scratch("nofence").unwrap(),
        "probe.md",
        "name: probe\n---\nUse for probing.",
    )
    .unwrap();
    expect(&path, "file must start with a --- line");
}

#[test]
fn missing_closing_fence_is_rejected() {
    let path = write(
        &scratch("noclose").unwrap(),
        "probe.md",
        "---\nname: probe\nUse for probing.",
    )
    .unwrap();
    expect(&path, "no closing --- line");
}

#[test]
fn empty_frontmatter_block_is_rejected() {
    let path = write(
        &scratch("emptyfm").unwrap(),
        "probe.md",
        "---\n---\nUse for probing.",
    )
    .unwrap();
    expect(&path, "frontmatter block is empty");
}

#[test]
fn tab_in_frontmatter_is_rejected() {
    let path = agent(
        "probe.md",
        "name:\tprobe\ndescription: Use for probing.\n",
        "Body.",
    )
    .unwrap();
    expect(&path, "tab character in frontmatter");
}

#[test]
fn blank_line_in_frontmatter_is_rejected() {
    let path = agent(
        "probe.md",
        "name: probe\n\ndescription: Use for probing.\n",
        "Body.",
    )
    .unwrap();
    expect(&path, "blank line in frontmatter");
}

#[test]
fn unparseable_line_is_rejected() {
    let path = agent(
        "probe.md",
        "name probe\ndescription: Use for probing.\n",
        "Body.",
    )
    .unwrap();
    expect(&path, "unparseable frontmatter line");
}

#[test]
fn continuation_before_any_key_is_rejected() {
    let path = agent("probe.md", "- bash\nname: probe\n", "Body.").unwrap();
    expect(&path, "list or continuation line with no key");
}

#[test]
fn duplicate_key_is_rejected() {
    let path = agent(
        "probe.md",
        "name: probe\nname: probe\ndescription: Use for probing.\n",
        "Body.",
    )
    .unwrap();
    expect(&path, "duplicate key: name");
}

#[test]
fn unknown_key_is_rejected() {
    let path = agent(
        "probe.md",
        "name: probe\ndescription: Use for probing.\nwhenToUse: Probing.\n",
        "Body.",
    )
    .unwrap();
    expect(&path, "unknown frontmatter key: whenToUse");
}

#[test]
fn crlf_file_parses_like_lf() {
    let path = write(
        &scratch("crlf").unwrap(),
        "probe.md",
        "---\r\nname: probe\r\ndescription: Use for probing.\r\n---\r\nBody line.\r\n",
    )
    .unwrap();
    let agent = load_file(&path).expect("a CRLF agent file must load");
    assert_eq!(agent.name, "probe");
    assert_eq!(agent.body, "Body line.");
}

#[test]
fn folded_description_joins_lines_with_single_spaces() {
    let path = agent(
        "probe.md",
        "name: probe\ndescription: >-\n  Use for audits\n  of landed work.\n",
        "Body.",
    )
    .unwrap();
    let agent = load_file(&path).expect("a folded description must load");
    assert_eq!(agent.description, "Use for audits of landed work.");
}

#[test]
fn list_items_fold_into_values() {
    let path = agent(
        "probe.md",
        "name: probe\ndescription: Use for probing.\ntools:\n  - bash\n  - read_file\n",
        "Body.",
    )
    .unwrap();
    let agent = load_file(&path).expect("list-form tools must load");
    assert_eq!(agent.tools, Some(vec!["bash".into(), "read_file".into()]));
}

#[test]
fn bracket_list_parses() {
    let path = agent(
        "probe.md",
        "name: probe\ndescription: Use for probing.\ntools: [bash, read_file]\n",
        "Body.",
    )
    .unwrap();
    let agent = load_file(&path).expect("bracket lists must load");
    assert_eq!(agent.tools, Some(vec!["bash".into(), "read_file".into()]));
}

#[test]
fn inline_value_without_space_parses() {
    let path = agent(
        "probe.md",
        "name: probe\ndescription: Use for probing.\ntools:bash\n",
        "Body.",
    )
    .unwrap();
    let agent = load_file(&path).expect("a colon-tight value must load");
    assert_eq!(agent.tools, Some(vec!["bash".into()]));
}

#[test]
fn defaults_apply_when_optional_fields_are_absent() {
    let path = agent(
        "probe.md",
        "name: probe\ndescription: Use for probing.\n",
        "Body.",
    )
    .unwrap();
    let agent = load_file(&path).expect("a minimal file must load");
    assert_eq!(agent.model, None);
    assert_eq!(agent.prompt_mode, PromptMode::Extend);
    assert_eq!(agent.tools, None);
    assert!(agent.disallowed_tools.is_empty());
    assert_eq!(agent.max_turns, None);
    assert_eq!(agent.timeout_secs, None);
    assert!(!agent.background);
    assert!(agent.agents_md);
}

#[test]
fn body_is_edge_trimmed() {
    let path = agent(
        "probe.md",
        "name: probe\ndescription: Use for probing.\n",
        "\n\nBody.\n\n",
    )
    .unwrap();
    let agent = load_file(&path).expect("padded bodies must load");
    assert_eq!(agent.body, "Body.");
}
