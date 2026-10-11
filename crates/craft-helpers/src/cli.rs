//! CLI surface: argv → typed command configs.

use std::path::PathBuf;

use clap::error::Error as ClapError;
use clap::{Args, Parser, Subcommand};

use crate::dups::{MAX_CANDIDATES, MAX_MIN_LINES};
use crate::error::{HelperError, bounded};

pub const DEFAULT_LIMIT_PER_REPO: usize = 30;
pub const DEFAULT_MAX_PRS: usize = 50;
pub const DEFAULT_MIN_LINES: usize = 6;
pub const DEFAULT_MAX_CANDIDATES: usize = 20;
/// Hostile-input bound: a look-back window may not exceed one leap year.
pub const MAX_WINDOW_SECS: u64 = 366 * 24 * 3600;
const MAX_NAME_CHARS: usize = 100;

#[derive(Parser, Debug)]
#[command(
    name = "craft-helpers",
    version,
    about = "Deterministic collection helpers for the digest mission"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Collect merged pull requests per repository.
    Prs(PrsArgs),
    /// Cross-tree duplicate-code candidates.
    Dups(DupsArgs),
}

#[derive(Args, Debug)]
pub struct PrsArgs {
    /// GitHub org whose repositories are covered.
    #[arg(long, value_parser = parse_name)]
    pub org: String,
    /// Repositories to cover, comma-separated.
    #[arg(long, value_delimiter = ',', value_parser = parse_name, required = true)]
    pub repos: Vec<String>,
    /// Look-back window before now: <n><s|m|h|d>, at most 366d.
    #[arg(long, value_parser = parse_lookback)]
    pub since: Lookback,
    /// Newest merged PRs kept per repository.
    #[arg(long, default_value_t = DEFAULT_LIMIT_PER_REPO, value_parser = parse_limit_per_repo)]
    pub limit_per_repo: usize,
    /// Total merged PRs kept across all repositories.
    #[arg(long, default_value_t = DEFAULT_MAX_PRS, value_parser = parse_max_prs)]
    pub max_prs: usize,
    /// Emit the report as one JSON document.
    #[arg(long, required = true)]
    pub json: bool,
}

/// A parsed look-back window: seconds plus the label the report echoes.
#[derive(Clone, Debug)]
pub struct Lookback {
    pub secs: u64,
    pub label: String,
}

/// Parses argv into the typed CLI; clap errors carry help/version semantics.
pub fn parse_cli<I, T>(argv: I) -> Result<Cli, ClapError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::try_parse_from(argv)
}

fn invalid_arg(name: &'static str, value: &str) -> HelperError {
    HelperError::InvalidArg {
        name: name.to_string(),
        value: bounded(value),
    }
}

/// Org and repository names: URL-path-safe characters only.
fn parse_name(raw: &str) -> Result<String, HelperError> {
    let valid = !raw.is_empty()
        && raw.len() <= MAX_NAME_CHARS
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if valid {
        Ok(raw.to_string())
    } else {
        Err(invalid_arg("name", raw))
    }
}

/// `<n><s|m|h|d>` → a bounded look-back window; anything else is invalid.
pub fn parse_lookback(raw: &str) -> Result<Lookback, HelperError> {
    let invalid = || HelperError::InvalidWindow(raw.to_string());
    let Some((split, unit)) = raw.char_indices().next_back() else {
        return Err(invalid());
    };
    let Some(digits) = raw.get(..split) else {
        return Err(invalid());
    };
    let count: u64 = digits.parse().map_err(|_| invalid())?;
    let per_unit = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 24 * 3600,
        _ => return Err(invalid()),
    };
    let Some(secs) = count.checked_mul(per_unit) else {
        return Err(invalid());
    };
    if secs > MAX_WINDOW_SECS {
        return Err(invalid());
    }
    Ok(Lookback {
        secs,
        label: raw.to_string(),
    })
}

fn parse_bounded(
    raw: &str,
    name: &'static str,
    low: usize,
    high: usize,
) -> Result<usize, HelperError> {
    let value: usize = raw.parse().map_err(|_| invalid_arg(name, raw))?;
    if value < low || value > high {
        return Err(invalid_arg(name, raw));
    }
    Ok(value)
}

pub fn parse_limit_per_repo(raw: &str) -> Result<usize, HelperError> {
    parse_bounded(raw, "limit-per-repo", 1, crate::prs_run::MAX_LIMIT_PER_REPO)
}

pub fn parse_max_prs(raw: &str) -> Result<usize, HelperError> {
    parse_bounded(raw, "max-prs", 1, crate::prs_run::MAX_MAX_PRS)
}

pub fn parse_min_lines(raw: &str) -> Result<usize, HelperError> {
    let value: usize = raw.parse().map_err(|_| invalid_arg("min-lines", raw))?;
    if !(2..=MAX_MIN_LINES).contains(&value) {
        return Err(HelperError::InvalidMinLines(value));
    }
    Ok(value)
}

pub fn parse_max_candidates(raw: &str) -> Result<usize, HelperError> {
    parse_bounded(raw, "max-candidates", 1, MAX_CANDIDATES)
}

#[derive(Args, Debug)]
pub struct DupsArgs {
    /// Trees to scan, comma-separated.
    #[arg(long, value_delimiter = ',', required = true)]
    pub roots: Vec<PathBuf>,
    /// Consecutive normalized lines that count as a duplicate block.
    #[arg(long, default_value_t = DEFAULT_MIN_LINES, value_parser = parse_min_lines)]
    pub min_lines: usize,
    /// Duplicate groups kept, ranked by repeated lines.
    #[arg(long, default_value_t = DEFAULT_MAX_CANDIDATES, value_parser = parse_max_candidates)]
    pub max_candidates: usize,
    /// Emit the report as one JSON document.
    #[arg(long, required = true)]
    pub json: bool,
}
