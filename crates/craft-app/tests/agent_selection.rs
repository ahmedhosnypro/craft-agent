//! RED corpus: `--agent` selection — CLI args, profile resolution,
//! prompt and registry application, cap clamping, and the wall-clock
//! ending.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use craft_app::agent_select::{AgentSelectError, RunWatch, registry, resolve, system_prompt};
use craft_app::cli::{CliCommand, parse_cli};
use craft_app::run_cmd::{DEFAULT_SYSTEM_PROMPT, build_run_config};
use craft_app::{CancelCategory, RuntimeEvent, Usage};
use craft_core::agents::AgentFile;
use craft_core::agents::PromptMode;

fn scratch(name: &str) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "craft-app-agents-{}-{name}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A minimal repo root: a `.git` marker and an agents directory.
fn repo(name: &str) -> std::io::Result<PathBuf> {
    let dir = scratch(name)?;
    fs::write(dir.join(".git"), "gitdir: placeholder\n")?;
    fs::create_dir_all(dir.join(".craft-agent").join("agents"))?;
    Ok(dir)
}

fn profile(name: &str, tools: Option<Vec<&str>>, deny: &[&str], mode: PromptMode) -> AgentFile {
    AgentFile {
        path: PathBuf::from(format!("{name}.md")),
        name: name.to_string(),
        description: "Use for probing.".to_string(),
        model: None,
        prompt_mode: mode,
        tools: tools.map(|list| list.iter().map(|tool| tool.to_string()).collect()),
        disallowed_tools: deny.iter().map(|tool| tool.to_string()).collect(),
        max_turns: None,
        timeout_secs: None,
        background: false,
        agents_md: true,
        body: "The body.".to_string(),
    }
}

fn run_cli(args: &[&str]) -> Result<craft_app::cli::RunCli, Box<dyn std::error::Error>> {
    let mut argv = vec!["craft-agent", "run", "--prompt", "-"];
    argv.extend_from_slice(args);
    let cli = parse_cli(argv)?;
    let CliCommand::Run(run) = cli.command else {
        return Err(String::from("expected the run subcommand").into());
    };
    Ok(run)
}

#[test]
fn agent_flag_parses() {
    let run = run_cli(&["--agent", "repo-scout"]).unwrap();
    assert_eq!(run.agent.as_deref(), Some("repo-scout"));
    assert_eq!(run.agent_file, None);
}

#[test]
fn agent_file_flag_parses() {
    let run = run_cli(&["--agent-file", "agents/probe.md"]).unwrap();
    assert_eq!(
        run.agent_file,
        Some(Path::new("agents/probe.md").to_path_buf())
    );
    assert_eq!(run.agent, None);
}

#[test]
fn agent_and_agent_file_conflict() {
    let err = parse_cli([
        "craft-agent",
        "run",
        "--prompt",
        "-",
        "--agent",
        "probe",
        "--agent-file",
        "probe.md",
    ])
    .expect_err("the two selection flags must conflict");
    assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn resolve_selects_nothing_without_flags() {
    let run = run_cli(&[]).unwrap();
    let agent = resolve(&run, &repo("bare").unwrap()).expect("no flags selects nothing");
    assert_eq!(agent, None);
}

#[test]
fn resolve_finds_the_named_profile() {
    let root = repo("found").unwrap();
    fs::write(
        root.join(".craft-agent").join("agents").join("probe.md"),
        "---\nname: probe\ndescription: Use for probing.\ntimeoutSecs: 600\n---\nBody text.\n",
    )
    .unwrap();
    let run = run_cli(&["--agent", "probe"]).unwrap();
    let agent = resolve(&run, &root).expect("the profile must resolve");
    let agent = agent.expect("a profile was selected");
    assert_eq!(agent.name, "probe");
    assert_eq!(agent.body, "Body text.");
    assert_eq!(agent.timeout_secs, Some(600));
}

#[test]
fn resolve_unknown_lists_the_catalog() {
    let root = repo("unknown").unwrap();
    fs::write(
        root.join(".craft-agent").join("agents").join("probe.md"),
        "---\nname: probe\ndescription: Use for probing.\n---\nBody.\n",
    )
    .unwrap();
    let run = run_cli(&["--agent", "other"]).unwrap();
    let err = resolve(&run, &root).expect_err("an unknown name must fail");
    let AgentSelectError::Unknown { name, available } = &err else {
        panic!("expected the unknown-profile error, got {err}");
    };
    assert_eq!(name, "other");
    assert_eq!(available, "probe");
}

#[test]
fn resolve_unknown_without_any_agents_dir_names_none() {
    let run = run_cli(&["--agent", "probe"]).unwrap();
    let err = resolve(&run, &repo("nodir").unwrap()).expect_err("no agents dir must fail lookup");
    let text = err.to_string();
    assert!(text.contains("unknown agent profile `probe`"), "{text}");
    assert!(text.contains("(none found)"), "{text}");
}

#[test]
fn resolve_surfaces_the_selected_file_error() {
    let root = repo("invalid").unwrap();
    fs::write(
        root.join(".craft-agent").join("agents").join("probe.md"),
        "---\nname: probe\ndescription: Use for probing.\nwhenToUse: never\n---\nBody.\n",
    )
    .unwrap();
    let run = run_cli(&["--agent", "probe"]).unwrap();
    let err = resolve(&run, &root).expect_err("an invalid selected file must fail the run");
    let AgentSelectError::File { source } = &err else {
        panic!("expected the invalid-file error, got {err}");
    };
    assert!(
        source.to_string().contains("unknown frontmatter key"),
        "{source}"
    );
}

#[test]
fn agent_file_is_fail_loud() {
    let dir = scratch("failloud").unwrap();
    let path = dir.join("probe.md");
    fs::write(
        &path,
        "---\nname: probe\ndescription: Use for probing.\nmodel: BAD\n---\nBody.\n",
    )
    .unwrap();
    let run = run_cli(&["--agent-file", path.to_str().unwrap()]).unwrap();
    let err = resolve(&run, &dir).expect_err("an invalid explicit file must fail");
    let AgentSelectError::File { source } = &err else {
        panic!("expected the invalid-file error, got {err}");
    };
    assert!(source.to_string().contains("MODEL01"), "{source}");
}

#[test]
fn registry_filters_the_allowlist_minus_the_denylist() {
    let agent = profile(
        "probe",
        Some(vec!["read_file", "glob"]),
        &["glob"],
        PromptMode::Full,
    );
    assert_eq!(registry(Some(&agent)).names(), ["read_file"]);
}

#[test]
fn registry_inherits_everything_without_a_tools_field() {
    let agent = profile("probe", None, &["bash", "gh"], PromptMode::Full);
    assert_eq!(
        registry(Some(&agent)).names(),
        ["edit_file", "glob", "grep", "read_file", "write_file"]
    );
}

#[test]
fn registry_without_a_profile_mounts_every_builtin() {
    assert_eq!(registry(None).names().len(), 7);
}

#[test]
fn system_prompt_full_replaces_and_extend_prepends() {
    let full = profile("probe", None, &[], PromptMode::Full);
    assert_eq!(system_prompt(Some(&full)), "The body.");
    let extend = profile("probe", None, &[], PromptMode::Extend);
    let prompt = system_prompt(Some(&extend));
    assert!(prompt.starts_with(DEFAULT_SYSTEM_PROMPT));
    assert!(prompt.ends_with("\n\nThe body."));
    assert_eq!(system_prompt(None), DEFAULT_SYSTEM_PROMPT);
}

#[test]
fn the_parent_turn_cap_wins_when_both_are_set() {
    let agent = profile("probe", None, &[], PromptMode::Full);
    let parent = run_cli(&["--max-turns", "50"]).unwrap();
    let config = build_run_config(&parent, Some(&agent), PathBuf::from("/w"), PathBuf::new());
    assert_eq!(config.max_turns, 50);
}

#[test]
fn the_agent_turn_cap_applies_when_the_parent_sets_none() {
    let agent = profile("probe", None, &[], PromptMode::Full);
    let run = run_cli(&[]).unwrap();
    let config = build_run_config(&run, Some(&agent), PathBuf::from("/w"), PathBuf::new());
    assert_eq!(config.max_turns, 40);
}

#[test]
fn an_agent_turn_cap_lower_than_the_default_applies() {
    let agent = AgentFile {
        max_turns: Some(15),
        ..profile("probe", None, &[], PromptMode::Full)
    };
    let run = run_cli(&[]).unwrap();
    let config = build_run_config(&run, Some(&agent), PathBuf::from("/w"), PathBuf::new());
    assert_eq!(config.max_turns, 15);
}

#[test]
fn wall_clock_expiry_ends_the_run_with_an_incomplete_record() {
    let watch = Arc::new(Mutex::new(RunWatch::default()));
    for turn in 1..=2 {
        RunWatch::record(
            &watch,
            &RuntimeEvent::TurnUsage {
                turn,
                usage: Usage {
                    input_tokens: 10 * u64::from(turn),
                    output_tokens: 4 * u64::from(turn),
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    reasoning_tokens: 0,
                    incomplete: false,
                },
            },
        );
    }
    let (outcome, event) = RunWatch::expire(&watch, 600, Some(Path::new("/w/session.jsonl")));

    assert_eq!(outcome.exit_code(), 1);
    assert_eq!(outcome.turns, 2);
    assert!(outcome.usage.incomplete);
    assert_eq!(outcome.usage.input_tokens, 20);
    assert_eq!(
        outcome.session_path,
        Some(PathBuf::from("/w/session.jsonl"))
    );
    let craft_app::RuntimeEvent::RunFinished {
        exit_code,
        category,
        detail,
        usage,
        turns,
    } = event
    else {
        panic!("expected one run_finished event");
    };
    assert_eq!(exit_code, 1);
    assert_eq!(category, CancelCategory::Error);
    assert!(detail.contains("wall-clock cap (600s)"), "{detail}");
    assert!(usage.incomplete);
    assert_eq!(turns, 2);
}
