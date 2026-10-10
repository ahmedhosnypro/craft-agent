# 01 — Architecture

craft-agent is a Cargo workspace of six crates:

- `craft-core` — the neutral model vocabulary: request shapes, the model
  event union (text deltas, tool-call deltas, turn-finish with stop reason
  and usage), usage accounting, and the streaming `Provider` trait every
  adapter implements.
- `craft-providers` — the three adapters (Anthropic Messages, OpenAI Chat
  Completions, Gemini generateContent) behind a flat `Backend` enum.
  Dialect-as-data: the wire differences live in the adapters, never in the
  callers.
- `craft-tools` — the `Tool` trait (schema, execution, the
  progress-then-terminal event contract) and the builtin tools.
- `craft-runtime` — the run loop: turn scheduling, budget caps
  (max-turns, max-estimated-cost), the append-only session record, and the
  stream-json output contract.
- `craft-digest` — the digest mission: deterministic collectors, prompt
  assembly, and publish preparation.
- `craft-app` — the `craft-agent` CLI binary wiring the crates into the
  `run` and `digest` commands.

The agent is headless: one process, typed commands, NDJSON output, exit
codes 0/1. No TUI, no plugins.
