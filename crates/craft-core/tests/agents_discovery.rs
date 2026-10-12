//! RED corpus: agent discovery — closest `.craft-agent/agents` from
//! the start up to the git root, fail-soft directory scans, catalog
//! lookup, and the seeded profile files.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use craft_core::agents::{
    AgentFileError, CatalogHit, discover_dir, load_catalog, load_dir, load_file,
};

fn scratch(name: &str) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "craft-agents-d-{}-{name}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn write(path: &Path, text: &str) -> std::io::Result<()> {
    fs::write(path, text)
}

fn agents_dir(root: &Path) -> std::io::Result<PathBuf> {
    let dir = root.join(".craft-agent").join("agents");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn profile(name: &str, description: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\nBody of {name}.\n")
}

fn make_repo(root: &Path) -> std::io::Result<()> {
    fs::create_dir_all(root)?;
    write(&root.join(".git"), "gitdir: placeholder\n")
}

#[test]
fn closest_agents_dir_wins() {
    let base = scratch("closest").unwrap();
    let root = base.join("repo");
    make_repo(&root).unwrap();
    let outer = agents_dir(&root).unwrap();
    write(&outer.join("outer.md"), &profile("outer", "Use for outer.")).unwrap();
    let sub = root.join("sub");
    let inner = agents_dir(&sub).unwrap();
    write(&inner.join("inner.md"), &profile("inner", "Use for inner.")).unwrap();

    assert_eq!(discover_dir(&sub), Some(inner.clone()));
    assert_eq!(discover_dir(&root), Some(outer));
    assert_eq!(discover_dir(&sub.join("deeper")), Some(inner));
}

#[test]
fn discovery_stops_at_the_git_root() {
    let base = scratch("rootstop").unwrap();
    let outer = agents_dir(&base).unwrap();
    write(&outer.join("above.md"), &profile("above", "Use for above.")).unwrap();
    let repo = base.join("repo");
    make_repo(&repo).unwrap();
    let deep = repo.join("a").join("b");
    fs::create_dir_all(&deep).unwrap();

    assert_eq!(discover_dir(&deep), None);
}

#[test]
fn git_root_hosts_the_last_candidate() {
    let base = scratch("rootcandidate").unwrap();
    let root = base.join("repo");
    make_repo(&root).unwrap();
    let outer = agents_dir(&root).unwrap();
    write(
        &outer.join("rooted.md"),
        &profile("rooted", "Use for root."),
    )
    .unwrap();
    let deep = root.join("a").join("b").join("c");
    fs::create_dir_all(&deep).unwrap();

    assert_eq!(discover_dir(&deep), Some(outer));
}

#[test]
fn discovery_is_depth_capped() {
    let base = scratch("depth").unwrap();
    let mut deep = base.clone();
    for _ in 0..9 {
        deep = deep.join("d");
        fs::create_dir_all(&deep).unwrap();
    }
    let top = base.join(".craft-agent").join("agents");
    fs::create_dir_all(&top).unwrap();
    write(&top.join("far.md"), &profile("far", "Use for far.")).unwrap();

    assert_eq!(discover_dir(&deep), None);
}

#[test]
fn load_dir_skips_invalid_files_fail_soft() {
    let dir = scratch("failsoft")
        .unwrap()
        .join(".craft-agent")
        .join("agents");
    fs::create_dir_all(&dir).unwrap();
    write(&dir.join("good.md"), &profile("good", "Use for good.")).unwrap();
    write(
        &dir.join("bad.md"),
        "---\nname: bad\ndescription: Use for bad.\nwhenToUse: never\n---\nBody.\n",
    )
    .unwrap();
    let catalog = load_dir(&dir).expect("the directory must load");
    assert_eq!(catalog.names(), ["good"]);
    assert_eq!(catalog.skipped.len(), 1);
    assert_eq!(catalog.skipped[0].key, "bad");
    assert!(matches!(
        catalog.skipped[0].error,
        AgentFileError::Frontmatter { .. }
    ));
}

#[test]
fn lookup_reports_found_invalid_and_missing() {
    let dir = scratch("lookup")
        .unwrap()
        .join(".craft-agent")
        .join("agents");
    fs::create_dir_all(&dir).unwrap();
    write(&dir.join("good.md"), &profile("good", "Use for good.")).unwrap();
    write(
        &dir.join("bad.md"),
        "---\nname: bad\ndescription: Use for bad.\nwhenToUse: never\n---\nBody.\n",
    )
    .unwrap();
    let catalog = load_dir(&dir).expect("the directory must load");

    assert!(matches!(catalog.lookup("good"), CatalogHit::Found(_)));
    assert!(matches!(catalog.lookup("bad"), CatalogHit::Invalid(_)));
    assert_eq!(catalog.lookup("other"), CatalogHit::Missing);
}

#[test]
fn invalid_selection_is_reachable_by_parsed_name() {
    let dir = scratch("badkey")
        .unwrap()
        .join(".craft-agent")
        .join("agents");
    fs::create_dir_all(&dir).unwrap();
    write(
        &dir.join("mismatch.md"),
        "---\nname: wrong\ndescription: Use for wrong.\n---\nBody.\n",
    )
    .unwrap();
    let catalog = load_dir(&dir).expect("the directory must load");
    assert!(matches!(catalog.lookup("wrong"), CatalogHit::Invalid(_)));
    assert!(matches!(catalog.lookup("mismatch"), CatalogHit::Invalid(_)));
}

#[test]
fn non_markdown_and_subdirectories_are_ignored() {
    let dir = scratch("ignored")
        .unwrap()
        .join(".craft-agent")
        .join("agents");
    fs::create_dir_all(&dir).unwrap();
    write(&dir.join("agent.txt"), &profile("txt", "Use for txt.")).unwrap();
    write(
        &dir.join(".hidden.md"),
        &profile("hidden", "Use for hidden."),
    )
    .unwrap();
    let nested = dir.join("nested.md");
    fs::create_dir_all(&nested).unwrap();
    write(&dir.join("real.md"), &profile("real", "Use for real.")).unwrap();

    let catalog = load_dir(&dir).expect("the directory must load");
    assert_eq!(catalog.names(), ["real"]);
    assert_eq!(catalog.skipped.len(), 1);
    assert_eq!(catalog.skipped[0].key, "hidden");
    assert_eq!(catalog.skipped[0].stem, ".hidden");
}

#[test]
fn missing_agents_dir_is_an_empty_catalog() {
    let start = scratch("empty-start").unwrap();
    let catalog = load_catalog(&start).expect("no agents dir is not an error");
    assert!(catalog.agents.is_empty());
    assert!(catalog.skipped.is_empty());
}

#[test]
fn unreadable_agents_dir_fails_closed() {
    let dir = scratch("unreadable").unwrap().join("agents");
    let err = load_dir(&dir).expect_err("a missing directory read must fail closed");
    assert!(matches!(err, AgentFileError::Io { .. }));
}

#[test]
fn explicit_file_load_is_fail_closed() {
    let dir = scratch("explicit").unwrap();
    let path = dir.join("probe.md");
    write(
        &path,
        "---\nname: probe\ndescription: Use for probing.\nmodel: BAD\n---\nBody.\n",
    )
    .unwrap();
    let err = load_file(&path).expect_err("an invalid explicit file must fail");
    assert!(matches!(err, AgentFileError::Model { .. }));
}

#[test]
fn seeded_agent_files_load_clean() {
    let root = Path::new("../../");
    let Some(dir) = discover_dir(root) else {
        println!("SKIP: no .craft-agent/agents directory in this checkout");
        return;
    };
    let catalog = load_dir(&dir).expect("the seeded agents must load");
    if catalog.agents.is_empty() && catalog.skipped.is_empty() {
        println!("SKIP: no seeded agent files present");
        return;
    }
    assert_eq!(
        catalog.names(),
        ["docs-writer", "pr-analyst", "repo-scout"],
        "every seeded agent must load clean: {:?}",
        catalog.skipped
    );
    let scout = match catalog.lookup("repo-scout") {
        CatalogHit::Found(agent) => agent,
        _ => panic!("repo-scout must resolve"),
    };
    assert_eq!(
        scout.tools,
        Some(vec!["glob".into(), "grep".into(), "read_file".into()])
    );
    assert_eq!(
        scout.disallowed_tools,
        vec![
            "bash".to_string(),
            "write_file".to_string(),
            "edit_file".to_string(),
            "gh".to_string()
        ]
    );
    assert_eq!(scout.max_turns, Some(15));
    assert_eq!(scout.timeout_secs, Some(600));
}
