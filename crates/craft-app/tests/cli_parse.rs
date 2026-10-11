//! RED corpus: CLI argument parsing — argv → typed config.

use craft_app::cli::{CliCommand, OutputFormat, PromptSource, parse_cli};

#[test]
fn run_without_prompt_is_missing_required_argument() {
    let err = parse_cli(["craft-agent", "run"]).expect_err("--prompt is required");
    assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
}

#[test]
fn run_prompt_stdin_parses() {
    let cli = parse_cli(["craft-agent", "run", "--prompt", "-"]).expect("minimal run argv");
    let CliCommand::Run(run) = cli.command else {
        panic!("expected run subcommand");
    };
    assert!(matches!(run.prompt, PromptSource::Stdin));
}

#[test]
fn run_prompt_file_parses() {
    let cli =
        parse_cli(["craft-agent", "run", "--prompt", "notes/goal.md"]).expect("file prompt parses");
    let CliCommand::Run(run) = cli.command else {
        panic!("expected run subcommand");
    };
    assert!(matches!(
        run.prompt,
        PromptSource::File(ref path) if path == std::path::Path::new("notes/goal.md")
    ));
}

#[test]
fn run_output_format_defaults_to_stream_json() {
    let cli = parse_cli(["craft-agent", "run", "--prompt", "-"]).expect("parses");
    let CliCommand::Run(run) = cli.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(run.output_format, OutputFormat::StreamJson);
}

#[test]
fn run_output_format_plain_is_accepted() {
    let cli = parse_cli([
        "craft-agent",
        "run",
        "--prompt",
        "-",
        "--output-format",
        "plain",
    ])
    .expect("--output-format plain parses");
    let CliCommand::Run(run) = cli.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(run.output_format, OutputFormat::Plain);
}

#[test]
fn run_output_format_unknown_rejected() {
    let err = parse_cli([
        "craft-agent",
        "run",
        "--prompt",
        "-",
        "--output-format",
        "xml",
    ])
    .expect_err("unknown output format must fail");
    assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
}

#[test]
fn run_caps_default_to_none() {
    let cli = parse_cli(["craft-agent", "run", "--prompt", "-"]).expect("parses");
    let CliCommand::Run(run) = cli.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(run.max_turns, None);
    assert_eq!(run.max_tokens, None);
    assert_eq!(run.idle_timeout_secs, None);
}

#[test]
fn run_caps_parse_when_given() {
    let argv = [
        "craft-agent",
        "run",
        "--prompt",
        "-",
        "--max-turns",
        "7",
        "--max-tokens",
        "123456",
        "--idle-timeout-secs",
        "30",
    ];
    let cli = parse_cli(argv).expect("caps parse");
    let CliCommand::Run(run) = cli.command else {
        panic!("expected run subcommand");
    };
    assert_eq!(run.max_turns, Some(7));
    assert_eq!(run.max_tokens, Some(123_456u64));
    assert_eq!(run.idle_timeout_secs, Some(30));
}

#[test]
fn run_negative_cap_is_rejected() {
    // The `=` form reaches clap's value parser; a bare `-1` would read as
    // an unknown argument instead.
    let err = parse_cli(["craft-agent", "run", "--prompt", "-", "--max-turns=-1"])
        .expect_err("negative cap must fail");
    assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn unknown_flag_is_rejected() {
    let err = parse_cli(["craft-agent", "run", "--prompt", "-", "--bogus"])
        .expect_err("unknown flag must fail");
    assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
}

#[test]
fn digest_defaults_apply() {
    let cli = parse_cli(["craft-agent", "digest"]).expect("digest parses bare");
    let CliCommand::Digest(d) = cli.command else {
        panic!("expected digest subcommand");
    };
    assert_eq!(d.since, std::time::Duration::from_secs(24 * 3600));
    assert_eq!(d.org, "storytold");
    assert_eq!(d.repos, None);
    assert!(!d.dry_run);
    assert_eq!(d.max_turns, None);
}

#[test]
fn digest_since_window_parses() {
    let cli = parse_cli(["craft-agent", "digest", "--since", "30m"]).expect("window parses");
    let CliCommand::Digest(d) = cli.command else {
        panic!("expected digest subcommand");
    };
    assert_eq!(d.since, std::time::Duration::from_secs(1800));
}

#[test]
fn digest_since_window_rejects_garbage() {
    let err = parse_cli(["craft-agent", "digest", "--since", "whenever"])
        .expect_err("bad window must fail");
    assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
}

#[test]
fn digest_org_repos_and_dry_run_parse() {
    let argv = [
        "craft-agent",
        "digest",
        "--org",
        "acme",
        "--repos",
        "a,b",
        "--dry-run",
    ];
    let cli = parse_cli(argv).expect("digest flags parse");
    let CliCommand::Digest(d) = cli.command else {
        panic!("expected digest subcommand");
    };
    assert_eq!(d.org, "acme");
    assert_eq!(d.repos, Some(vec!["a".to_string(), "b".to_string()]));
    assert!(d.dry_run);
}

#[test]
fn parse_window_units_and_bounds() {
    let ninety = craft_app::cli::parse_window("90s").expect("seconds parse");
    assert_eq!(ninety, std::time::Duration::from_secs(90));
    let two_days = craft_app::cli::parse_window("2d").expect("days parse");
    assert_eq!(two_days, std::time::Duration::from_secs(2 * 24 * 3600));
    for bad in ["", "12", "whenever", "999999999d", "1x"] {
        assert!(
            craft_app::cli::parse_window(bad).is_err(),
            "`{bad}` must not parse as a window"
        );
    }
}

#[test]
fn missing_subcommand_shows_help() {
    let err = parse_cli(["craft-agent"]).expect_err("bare craft-agent must fail");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
}

#[test]
fn parse_prompt_source_dash_is_stdin_else_file() {
    let stdin = craft_app::cli::parse_prompt_source("-").expect("dash parses");
    assert!(matches!(stdin, PromptSource::Stdin));
    let file = craft_app::cli::parse_prompt_source("p.md").expect("path parses");
    assert!(matches!(file, PromptSource::File(ref p) if p == std::path::Path::new("p.md")));
}

#[test]
fn digest_accepts_model_turn_caps() {
    let argv = [
        "craft-agent",
        "digest",
        "--max-turns",
        "3",
        "--idle-timeout-secs",
        "60",
    ];
    let cli = parse_cli(argv).expect("digest model-turn caps parse");
    let CliCommand::Digest(d) = cli.command else {
        panic!("expected digest subcommand");
    };
    assert_eq!(d.max_turns, Some(3));
    assert_eq!(d.idle_timeout_secs, Some(60));
}
