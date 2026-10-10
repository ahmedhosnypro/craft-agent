//! craft-agent: the BYOK agentic AI agent for the storytold suite.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "craft-agent",
    version,
    about = "BYOK agentic AI agent for the storytold suite"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the agent over a prompt.
    Run,
    /// Produce the storytold suite digest.
    Digest,
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("the `{command}` command is not implemented yet")]
    Unsupported { command: &'static str },
}

fn run_agent() -> Result<(), AppError> {
    Err(AppError::Unsupported { command: "run" })
}

fn run_digest() -> Result<(), AppError> {
    Err(AppError::Unsupported { command: "digest" })
}

fn main() {
    let cli = Cli::parse();
    let outcome = match cli.command {
        Command::Run => run_agent(),
        Command::Digest => run_digest(),
    };
    if let Err(error) = outcome {
        eprintln!("craft-agent: {error}");
        std::process::exit(1);
    }
}
