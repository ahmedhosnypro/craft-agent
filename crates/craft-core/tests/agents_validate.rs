//! RED corpus: agent-file field rules — required identity, name
//! charset and stem, description shape and trigger, model shape, and
//! tool ids; each row mirrors a rule of the agent-file gate.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use craft_core::agents::{AgentFileError, load_file};

fn scratch(name: &str) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "craft-agents-v-{}-{name}-{unique}",
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
fn name_is_required() {
    let dir = scratch("noname").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\ndescription: Use for probing.\n---\nBody.\n",
    )
    .unwrap();
    expect(&path, "missing required field: name");
}

#[test]
fn empty_name_is_rejected() {
    let dir = scratch("emptyname").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname:\ndescription: Use for probing.\n---\nBody.\n",
    )
    .unwrap();
    expect(&path, "name is empty");
}

#[test]
fn description_is_required() {
    let dir = scratch("nodesc").unwrap();
    let path = write(&dir, "probe.md", "---\nname: probe\n---\nBody.\n").unwrap();
    expect(&path, "missing required field: description");
}

#[test]
fn empty_description_is_rejected() {
    let dir = scratch("emptydesc").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: probe\ndescription:\n---\nBody.\n",
    )
    .unwrap();
    expect(&path, "description is empty");
}

#[test]
fn uppercase_name_is_rejected() {
    let dir = scratch("upper").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: Probe\ndescription: Use for probing.\n---\nBody.\n",
    )
    .unwrap();
    expect(&path, "name must be a lowercase slug");
}

#[test]
fn overlong_name_is_rejected() {
    let dir = scratch("longname").unwrap();
    let long = format!("a{}.md", "b".repeat(64));
    let path = write(
        &dir,
        &long,
        &format!(
            "---\nname: a{}\ndescription: Use for probing.\n---\nBody.\n",
            "b".repeat(64)
        ),
    )
    .unwrap();
    expect(&path, "name must be a lowercase slug");
}

#[test]
fn stem_mismatch_is_rejected() {
    let dir = scratch("stem").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: other\ndescription: Use for probing.\n---\nBody.\n",
    )
    .unwrap();
    expect(&path, "name 'other' must equal the file stem 'probe'");
}

#[test]
fn placeholder_description_is_rejected() {
    let dir = scratch("placeholder").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: probe\ndescription: TBD\n---\nBody.\n",
    )
    .unwrap();
    expect(&path, "placeholder description");
}

#[test]
fn oversized_description_is_rejected() {
    let dir = scratch("bigdesc").unwrap();
    let long = "x".repeat(1025);
    let path = write(
        &dir,
        "probe.md",
        &format!("---\nname: probe\ndescription: {long}\n---\nBody.\n"),
    )
    .unwrap();
    expect(&path, "description is 1025 bytes (cap 1024)");
}

#[test]
fn missing_trigger_word_is_rejected() {
    let dir = scratch("notrigger").unwrap();
    let path = write(
        &dir,
        "probe.md",
        "---\nname: probe\ndescription: A quiet auditor.\n---\nBody.\n",
    )
    .unwrap();
    expect(&path, "description must say when to use the agent");
}

#[test]
fn bad_model_is_rejected() {
    let path = agent("probe.md", "model: Not A Model\n").unwrap();
    expect(&path, "model must be inherit or a bare lowercase id");
}

#[test]
fn unknown_tool_id_is_rejected() {
    let path = agent("probe.md", "tools: bash, read\n").unwrap();
    expect(&path, "unknown tool id 'read' in tools");
}

#[test]
fn unknown_disallowed_tool_id_is_rejected() {
    let path = agent("probe.md", "disallowedTools: shell\n").unwrap();
    expect(&path, "unknown tool id 'shell' in disallowedTools");
}

#[test]
fn unbalanced_brackets_are_rejected() {
    let path = agent("probe.md", "tools: [bash\n").unwrap();
    expect(&path, "unbalanced bracket list in tools");
}

#[test]
fn empty_tool_id_is_rejected() {
    let path = agent("probe.md", "tools: bash,,grep\n").unwrap();
    expect(&path, "empty tool id in tools");
}

#[test]
fn empty_tool_list_is_rejected() {
    let path = agent("probe.md", "tools: []\n").unwrap();
    expect(&path, "empty tool list in tools");
}

#[test]
fn bare_empty_tools_value_is_rejected() {
    let path = agent("probe.md", "tools:\n").unwrap();
    expect(&path, "empty tool list in tools");
}

#[test]
fn missing_explicit_file_is_a_typed_io_error() {
    let missing = scratch("missing").unwrap().join("probe.md");
    let err = load_file(&missing).expect_err("a missing file must fail closed");
    assert!(matches!(err, AgentFileError::Io { .. }));
    assert!(err.to_string().contains("cannot read"));
}
