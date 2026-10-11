# craft-agent — instructions for agents

craft-agent is the suite's bring-your-own-key agentic AI agent: a pure-Rust
Cargo workspace behind one CLI, packaged as a GitHub Action. The v1 mission
is the daily suite digest — merged-PR summaries, common patterns, and
duplicate-code candidates across the storytold repos.

**Read [`AGENTS.md`](AGENTS.md) first; its rules override everything here.**
That file holds the never-break laws; this one holds how to work.

## Toolchain

`rust-toolchain.toml` pins the exact channel (rustfmt + clippy included) and
rustup provisions it automatically on the first build — build through rustup
so the pin applies, and never widen it. [just](https://github.com/casey/just)
runs everything else; GitHub runners lack it, so every gate recipe below is
mirrored verbatim in `.github/workflows/ci.yml`.

## The gate

`just check` before every commit — nothing is done until it is green:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` — the keyless corpus; live rows are `#[ignore]`d
- `./scripts/scan-secrets.sh` and `--selftest` (secret patterns + the
  tracked-`.env` rule)
- `./scripts/identifier-scan.sh` and `--selftest` (the no-lineage gate)
- `./scripts/check-file-length.sh` (≤300 effective lines per Rust file,
  tests included, no exemptions)
- `cargo deny --locked check` (licences, advisories, sources)
- `cargo machete` (zero unused dependencies)
- `cargo build --locked`

Both scanners fail closed — a scan that cannot run is a red gate, never a
silent pass — and each `--selftest` canary proves them in both directions:
every banned pattern fires, and legal strings never trip. Rust files stay
under 300 effective lines (`check-file-length.sh`) and functions under 75
(`clippy.toml`). Every gate change lands in ci.yml in the same commit.

## RED first

Tests are authored before the implementation and land with it — a feature
without its corpus is not a feature. Keyless fixture corpora always run in
the gate; live `#[ignore]` corpora run through the `just integration*`
recipes.

## Working rules

- One task = one background subagent and one tracked row in `progress.db`
  (`scripts/progress-db.sql` bootstraps it): mark the row in_progress when
  starting, done when finished, and record findings as you go.
- File writes are chunked: at most 50 lines per write call.
- Conventional commits; stage explicit paths only — never `git add -A`.
- Never open a live `.env`; the example/sample/template forms are the only
  env files an agent may read.

## Where things live

- `crates/craft-core` — the neutral model vocabulary: request/event types,
  the Provider trait, SSE, retry, usage
- `crates/craft-providers` — the three protocol adapters
- `crates/craft-tools` — the Tool trait and the loop's builtins
- `crates/craft-runtime` — the run loop, budget caps, session, stream-json
  output contract
- `crates/craft-digest` — the digest mission: collectors, prompt, publish
  prep
- `crates/craft-app` — the CLI binary
- `docs/` — the numbered set, `01-architecture.md` through `06-testing.md`
- `scripts/` — the scanners and the progress-db bootstrap
- `config/` — curated lists; `digest-repos.txt` is the repo set the digest
  covers

## Live corpora

The live corpora talk to an OpenAI-compatible test gateway and read only the
`TEST_BYOK_*` knobs from the gitignored `.env` at the repo root
(`.env.example` documents the names and defaults). Every live test is
`#[ignore]`d and runs through its `just integration*` recipe; when any knob
is missing or empty the corpus prints its SKIP reason and passes, so the
gate and CI stay green without `.env` — and with `.env` they must pass for
real.

## Done

A task is done when its `progress.db` row is done, `just check` is green,
and its corpora pass — live corpora passing against the local `.env`,
SKIP-clean without it. Nothing less counts as done.
