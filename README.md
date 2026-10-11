# craft-agent

A bring-your-own-key (BYOK) agentic AI agent in pure Rust, delivered as
a GitHub Action for the storytold suite (the craft apps and their shared
infrastructure).

- **Three providers behind one streaming trait** — Anthropic Messages,
  OpenAI Chat Completions (base URL overridable, so any
  OpenAI-compatible API works), and Google Gemini generateContent.
- **A general tool loop** — bash, file read/write/edit, glob, grep, and
  `gh` — an agent core that can do anything a command line can.
- **The v1 mission** — the daily suite digest: merged-PR summaries,
  cross-repo patterns, and duplicate-code candidates across the curated
  repo list, published as a sticky issue. See
  [docs/04-digest.md](docs/04-digest.md).

## CLI

```
craft-agent run --prompt <file|->
               [--output-format stream-json|plain]
               [--max-turns <n>] [--max-tokens <n>] [--idle-timeout-secs <n>]
craft-agent digest [--since <n><s|m|h|d>] [--org <org>] [--repos <a,b>]
                   [--dry-run]
                   [--max-turns <n>] [--max-tokens <n>] [--idle-timeout-secs <n>]
```

`run` drives the agent over a prompt — a file, or `-` for stdin — and
emits runtime events; `--output-format stream-json` (the default) is the
documented NDJSON contract other agents consume. `digest` renders the
suite digest: the window defaults to `24h` (bounded at `366d`), the org
to `storytold`, the repo set comes from `config/digest-repos.txt`, and
`--dry-run` prints to stdout and publishes nothing.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | the run completed |
| 1 | an error |
| 2 | the turn cap was reached, or the run went stationary |
| 3 | the token budget was exhausted |
| 4 | a sensitive-path denial |

## Quickstart

Requirements: a Rust toolchain — rustup provisions the pinned channel in
`rust-toolchain.toml` automatically — and
[just](https://github.com/casey/just).

```sh
git clone https://github.com/storytold/craft-agent
cd craft-agent
just check
```

`just check` is the full gate — eleven stages: fmt, clippy, the keyless
test corpus, both scanners with their selftests, file length, cargo
deny, cargo machete, and a locked build. Nothing is done until it is
green.

## Live corpora

The fixture corpus needs no credentials. The live integration corpora
ride a real endpoint through a gitignored `.env` at the repo root — copy
the example and fill in three knobs:

```sh
cp .env.example .env

# TEST_BYOK_BASE_URL — OpenAI-compatible base URL, /v1 included
# TEST_BYOK_API_KEY — the gateway key; empty means the corpora skip
# TEST_BYOK_MODEL   — the model id the corpora request

just integration               # completions round-trip
just integration_tools         # tool round-trip
just integration_stream_tools  # streamed tool-delta assembly
just integration_auth          # a bad key lands a typed error
just integration_dialect_mount # a dialect mount over one base URL
just integration_agent_loop    # the full agent loop
```

A missing or empty knob prints its SKIP reason and passes, so CI —
which carries no `.env` — stays green; set-but-broken knobs fail loudly.
The corpus map is [docs/06-testing.md](docs/06-testing.md).

## The GitHub Action

The repo ships `action.yml`, a composite Action that downloads the release
binary for the runner triple, sha256-verifies it, and runs one mission:

```yaml
- uses: storytold/craft-agent@<pinned-sha>
  with:
    mission: digest
    provider: openai
    model: <model-id>
    github-token: ${{ github.token }}
```

Provider keys are never inputs: map them from secrets into the step env
(`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GOOGLE_API_KEY`). The full
input and output surface, the digest workflow, and the pinning rules are
in [docs/05-action.md](docs/05-action.md); operations live in
[docs/runbook-app.md](docs/runbook-app.md) and
[docs/runbook-secrets.md](docs/runbook-secrets.md).

## Docs

[01 architecture](docs/01-architecture.md) ·
[02 providers](docs/02-providers.md) ·
[03 tools and loop](docs/03-tools-and-loop.md) ·
[04 digest](docs/04-digest.md) ·
[05 Action](docs/05-action.md) ·
[06 testing](docs/06-testing.md)

## License

MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
