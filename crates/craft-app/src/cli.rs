//! CLI surface: argv → typed command configs.

use std::path::PathBuf;
use std::time::Duration;

use clap::error::Error as ClapError;
use clap::{Parser, Subcommand, ValueEnum};

use crate::error::AppError;

pub const DEFAULT_WINDOW: &str = "24h";
pub const DEFAULT_ORG: &str = "storytold";
/// Hostile-input bound: a look-back window may not exceed one leap year.
const MAX_WINDOW_SECS: u64 = 366 * 24 * 3600;

#[derive(Parser, Debug)]
#[command(
    name = "craft-agent",
    version,
    about = "BYOK agentic AI agent for the storytold suite"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// Run the agent over a prompt.
    Run(RunCli),
    /// Produce the storytold suite digest.
    Digest(DigestCli),
}

/// Where the run prompt comes from: `-` is stdin, anything else a file.
#[derive(Clone, Debug)]
pub enum PromptSource {
    Stdin,
    File(PathBuf),
}

/// Run-event output; stream-json is the machine contract, plain the
/// human-readable fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// One JSON runtime event per NDJSON line.
    StreamJson,
    /// Human-readable text lines.
    Plain,
}

#[derive(clap::Args, Debug)]
pub struct RunCli {
    /// Prompt source: a file path, or `-` for stdin.
    #[arg(long, value_parser = parse_prompt_source)]
    pub prompt: PromptSource,
    /// Output format for run events.
    #[arg(long, value_enum, default_value = "stream-json")]
    pub output_format: OutputFormat,
    /// Turn cap; default 40.
    #[arg(long)]
    pub max_turns: Option<u32>,
    /// Total token budget; default 4000000.
    #[arg(long)]
    pub max_tokens: Option<u64>,
    /// Cancel the run after this many idle seconds; default off.
    #[arg(long)]
    pub idle_timeout_secs: Option<u64>,
    /// Agent profile name from the closest .craft-agent/agents; the
    /// file's body becomes the system prompt.
    #[arg(long, conflicts_with = "agent_file")]
    pub agent: Option<String>,
    /// One agent file used directly; an invalid file fails the run.
    #[arg(long)]
    pub agent_file: Option<PathBuf>,
}

#[derive(clap::Args, Debug)]
pub struct DigestCli {
    /// Look-back window before now: <n><s|m|h|d>, at most 366d.
    #[arg(long, default_value = DEFAULT_WINDOW, value_parser = parse_window)]
    pub since: Duration,
    /// GitHub org whose repositories the digest covers.
    #[arg(long, default_value = DEFAULT_ORG)]
    pub org: String,
    /// Repositories to cover, comma-separated; default is the curated list.
    #[arg(long, value_delimiter = ',')]
    pub repos: Option<Vec<String>>,
    /// Render the digest without publishing it.
    #[arg(long)]
    pub dry_run: bool,
    /// Turn cap for the digest model turn; default 40.
    #[arg(long)]
    pub max_turns: Option<u32>,
    /// Token budget for the digest model turn; default 4000000.
    #[arg(long)]
    pub max_tokens: Option<u64>,
    /// Cancel the digest turn after this many idle seconds; default off.
    #[arg(long)]
    pub idle_timeout_secs: Option<u64>,
}

/// Parses argv into the typed CLI; clap errors carry help/version semantics.
pub fn parse_cli<I, T>(argv: I) -> Result<Cli, ClapError>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::try_parse_from(argv)
}

pub fn parse_prompt_source(raw: &str) -> Result<PromptSource, AppError> {
    if raw == "-" {
        Ok(PromptSource::Stdin)
    } else {
        Ok(PromptSource::File(PathBuf::from(raw)))
    }
}

/// `<n><s|m|h|d>` → a bounded look-back window; anything else is invalid.
pub fn parse_window(raw: &str) -> Result<Duration, AppError> {
    let invalid = || AppError::InvalidWindow {
        value: raw.to_string(),
    };
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
    Ok(Duration::from_secs(secs))
}
