//! RED corpus: RunOutcome → process exit codes, incl. categories.

use craft_app::{CancelCategory, ExitStatus, Usage};

fn outcome(exit: ExitStatus, category: CancelCategory) -> craft_app::RunOutcome {
    craft_app::RunOutcome {
        exit,
        category,
        detail: "fixture".to_string(),
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            incomplete: false,
        },
        turns: 2,
        session_path: None,
    }
}

#[test]
fn status_to_exit_code_mapping() {
    assert_eq!(ExitStatus::Ok.code(), 0);
    assert_eq!(ExitStatus::Error.code(), 1);
    assert_eq!(ExitStatus::MaxTurns.code(), 2);
    assert_eq!(ExitStatus::Stationarity.code(), 2);
    assert_eq!(ExitStatus::Budget.code(), 3);
    assert_eq!(ExitStatus::Denied.code(), 4);
}

#[test]
fn outcomes_carry_their_category_and_code() {
    let completed = outcome(ExitStatus::Ok, CancelCategory::Completed);
    assert_eq!(completed.exit_code(), 0);
    assert_eq!(completed.category, CancelCategory::Completed);

    let errored = outcome(ExitStatus::Error, CancelCategory::Error);
    assert_eq!(errored.exit_code(), 1);

    let capped = outcome(ExitStatus::MaxTurns, CancelCategory::MaxTurns);
    assert_eq!(capped.exit_code(), 2);

    let stuck = outcome(ExitStatus::Stationarity, CancelCategory::Stationarity);
    assert_eq!(stuck.exit_code(), 2);
    assert_ne!(stuck.category, capped.category);

    let over_budget = outcome(ExitStatus::Budget, CancelCategory::Budget);
    assert_eq!(over_budget.exit_code(), 3);

    let sensitive = outcome(ExitStatus::Denied, CancelCategory::SensitivePath);
    assert_eq!(sensitive.exit_code(), 4);
}

#[test]
fn workspace_escape_denials_share_code_four() {
    let escaped = outcome(ExitStatus::Denied, CancelCategory::EscapesWorkspace);
    assert_eq!(escaped.exit_code(), 4);
    assert_eq!(escaped.exit_code(), ExitStatus::Denied.code());
}
