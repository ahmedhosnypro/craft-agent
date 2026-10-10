# craft-agent

A bring-your-own-key (BYOK) agentic AI agent in pure Rust, delivered as a
GitHub Action for the storytold suite (filmcraft, photocraft, and the other
craft apps).

- Three providers behind one streaming trait: Anthropic Messages, OpenAI
  Chat Completions (base URL overridable, so any OpenAI-compatible API
  works), and Google Gemini generateContent.
- A general tool loop — bash, file read/write/edit, glob, grep, and `gh` —
  an agent core that can do anything a command line can.
- The v1 mission: the daily suite digest — merged-PR summaries, common
  patterns, and duplicate-code candidates across the suite's repositories,
  published as a sticky issue under the agent's own GitHub App identity.

**Status: scaffold (M0).** The workspace, quality gates, and live-test
seams are in place; the provider adapters, tool loop, digest pipeline, and
Action packaging land in the next milestones.

## Quickstart

Requirements: a Rust toolchain (rustup provisions the pinned version in
`rust-toolchain.toml` automatically) and [just](https://github.com/casey/just).

```sh
git clone https://github.com/storytold/craft-agent
cd craft-agent
just check
```

## Live corpora

The live integration corpora talk to an OpenAI-compatible test gateway and
read their knobs from a gitignored `.env` at the repo root:

```sh
cp .env.example .env    # then fill in the key
just integration              # completions round-trip
just integration_tools        # tool round-trip
just integration_stream_tools # streamed tool-delta assembly
just integration_auth         # bad-key typed error
just integration_dialect_mount
just integration_agent_loop   # full agent loop
```

When any `TEST_BYOK_*` knob is missing or empty the corpus SKIPs with a
printed reason — never fails — so CI (which carries no `.env`) stays green.

## License

MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
