//! craft-helpers: deterministic collection for the digest mission.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use craft_helpers::cli::{Command, Lookback, PrsArgs, parse_cli};
use craft_helpers::dups::{DupsQuery, scan_dups};
use craft_helpers::error::HelperError;
use craft_helpers::http::HttpFetch;
use craft_helpers::prs_run::{PrsQuery, collect_prs, resolve_token};
use serde::Serialize;

fn main() {
    let cli = match parse_cli(std::env::args_os()) {
        Ok(cli) => cli,
        Err(err) => err.exit(),
    };
    match execute(cli.command) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("craft-helpers: {error}");
            std::process::exit(1);
        }
    }
}

fn execute(command: Command) -> Result<i32, HelperError> {
    match command {
        Command::Prs(args) => {
            let token = resolve_token(|name| std::env::var(name).ok())?;
            let fetch = HttpFetch::new(&token)?;
            let report = collect_prs(&fetch, &prs_query(args)?);
            print_json(&report)
        }
        Command::Dups(args) => {
            let query = DupsQuery {
                roots: args.roots,
                min_lines: args.min_lines,
                max_candidates: args.max_candidates,
            };
            let report = scan_dups(&query)?;
            print_json(&report)
        }
    }
}

fn now_secs() -> Result<u64, HelperError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|span| span.as_secs())
        .map_err(|_| HelperError::Io("system clock is before the Unix epoch".into()))
}

fn prs_query(args: PrsArgs) -> Result<PrsQuery, HelperError> {
    let Lookback { secs, label } = args.since;
    Ok(PrsQuery {
        org: args.org,
        repos: args.repos,
        now_secs: now_secs()?,
        lookback_secs: secs,
        limit_per_repo: args.limit_per_repo,
        max_prs: args.max_prs,
        since_label: label,
    })
}

fn print_json<T: Serialize>(value: &T) -> Result<i32, HelperError> {
    write_json(&mut std::io::stdout().lock(), value)
}

fn write_json<T: Serialize>(out: &mut dyn Write, value: &T) -> Result<i32, HelperError> {
    let text =
        serde_json::to_string_pretty(value).map_err(|err| HelperError::Output(err.to_string()))?;
    writeln!(out, "{text}").map_err(|err| HelperError::Output(err.to_string()))?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ClosedPipe;

    impl Write for ClosedPipe {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("pipe closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("pipe closed"))
        }
    }

    #[test]
    fn failed_write_is_a_typed_error_not_a_panic() {
        let result = write_json(&mut ClosedPipe, &"report");
        assert!(matches!(result, Err(HelperError::Output(_))));
    }
}
