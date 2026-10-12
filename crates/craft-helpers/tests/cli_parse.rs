// cli corpus: bounded argv parsing for the look-back window.
use clap::CommandFactory;
use craft_helpers::cli::{
    Cli, Lookback, parse_limit_per_repo, parse_lookback, parse_max_candidates, parse_max_prs,
    parse_min_lines,
};
use craft_helpers::error::{HelperError, MESSAGE_CHARS};

#[test]
fn lookback_accepts_bounded_units() {
    assert_eq!(parse_lookback("366d").unwrap().secs, 366 * 24 * 3600);
    assert_eq!(parse_lookback("90m").unwrap().secs, 5_400);
    assert_eq!(parse_lookback("2h").unwrap().secs, 7_200);
    assert_eq!(parse_lookback("45s").unwrap().secs, 45);
    let Lookback { label, .. } = parse_lookback("2h").unwrap();
    assert_eq!(label, "2h");
}

#[test]
fn lookback_rejects_hostile_windows() {
    for raw in ["", "d", "10x", "999999d", "367d", "366d extra", "-5d"] {
        assert!(parse_lookback(raw).is_err(), "{raw}");
    }
}

#[test]
fn invalid_window_error_is_capped() {
    let raw = "9".repeat(MESSAGE_CHARS * 4) + "x";
    let Err(HelperError::InvalidWindow(value)) = parse_lookback(&raw) else {
        panic!("expected InvalidWindow for a hostile window");
    };
    assert!(value.chars().count() <= MESSAGE_CHARS);
}

#[test]
fn range_errors_name_the_accepted_bounds() {
    for raw in ["0", "501", "1000", "900"] {
        let err = parse_max_candidates(raw).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--max-candidates"), "{raw}: {text}");
        assert!(text.contains("1..=500"), "{raw}: {text}");
    }
    for raw in ["1", "201", "300"] {
        let err = parse_min_lines(raw).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--min-lines"), "{raw}: {text}");
        assert!(text.contains("2..=200"), "{raw}: {text}");
    }
    for raw in ["0", "101", "99999"] {
        let err = parse_limit_per_repo(raw).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--limit-per-repo"), "{raw}: {text}");
        assert!(text.contains("1..=100"), "{raw}: {text}");
    }
    for raw in ["0", "501", "99999"] {
        let err = parse_max_prs(raw).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("--max-prs"), "{raw}: {text}");
        assert!(text.contains("1..=500"), "{raw}: {text}");
    }
    for raw in ["2", "6", "200"] {
        assert!(parse_min_lines(raw).is_ok(), "{raw}");
    }
    for raw in ["1", "20", "500"] {
        assert!(parse_max_candidates(raw).is_ok(), "{raw}");
    }
}

#[test]
fn help_carries_defaults_and_caps() {
    let mut cmd = Cli::command();
    let dups = cmd.find_subcommand_mut("dups").expect("dups subcommand");
    let help = dups.render_help().to_string();
    assert!(help.contains("--cross-only"), "{help}");
    assert!(help.contains("span multiple roots"), "{help}");
    assert!(help.contains("2..=200"), "{help}");
    assert!(help.contains("at most 500"), "{help}");
    assert!(help.contains("[default: 6]"), "{help}");
    assert!(help.contains("[default: 20]"), "{help}");
    let prs = cmd.find_subcommand_mut("prs").expect("prs subcommand");
    let help = prs.render_help().to_string();
    assert!(help.contains("at most 100"), "{help}");
    assert!(help.contains("at most 500"), "{help}");
    assert!(help.contains("[default: 30]"), "{help}");
    assert!(help.contains("[default: 50]"), "{help}");
    assert!(help.contains("at most 366d"), "{help}");
}
