//! craft-agent: the BYOK agentic AI agent for the storytold suite.

use craft_app::cli::{CliCommand, parse_cli};
use craft_app::{digest_cmd, run_cmd};

fn main() {
    let cli = match parse_cli(std::env::args_os()) {
        Ok(cli) => cli,
        Err(err) => err.exit(),
    };
    let outcome = match cli.command {
        CliCommand::Run(run) => run_cmd::execute_run(run),
        CliCommand::Digest(digest) => digest_cmd::execute_digest(digest),
    };
    match outcome {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("craft-agent: {error}");
            std::process::exit(1);
        }
    }
}
