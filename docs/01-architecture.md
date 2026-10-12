# 01 — Architecture

craft-agent is a pure-Rust BYOK agentic runner shipped as a GitHub
Action. One process, typed commands, NDJSON output, typed exit codes:
the binary drives a prompt through a model + tool loop under explicit
caps, and every failure is a value the caller can act on — never a
panic, never a lost run.

## Workspace map

| Crate | Role |
|---|---|
| `craft-core` | Neutral model vocabulary: request shapes, the model event union (text deltas, tool-call deltas, turn finish with stop reason and usage), usage accounting, and the streaming `Provider` trait. |
| `craft-providers` | The three wire adapters — Anthropic Messages, OpenAI Chat Completions, Gemini generateContent — behind a flat `Backend` enum. |
| `craft-tools` | The `Tool` trait, the `Registry`, execution limits, and the builtin tools. |
| `craft-runtime` | The run loop: turn scheduling, budgets, the stationarity guard, the append-only session record, and the stream-json event contract. |
| `craft-digest` | The digest mission: deterministic collectors, prompt assembly, and publish preparation. |
| `craft-app` | The `craft-agent` CLI: typed commands (`run`, `digest`), environment-derived configuration, and the process entry point. |

## Seams

The crates compose through one typed chain:

app → `Backend` → `Provider` → `ModelEvent` → runner loop → `Registry` → tools

1. `craft-app` resolves `EnvConfig` from explicitly named environment
   variables — a missing or empty variable is a hard error that names
   itself — and mounts the `Backend` variant it selects.
2. `Backend` implements `Provider` by dispatch to the mounted adapter;
   the runtime never knows which wire it is talking to.
3. An adapter turns one `ModelRequest` into a `ModelEventStream`:
   `TextDelta`, `ToolCallDelta` fragments keyed by the provider's wire
   index, and exactly one `TurnFinished` carrying stop reason and usage.
4. `TurnFold` assembles the stream into a `FoldedTurn`, fail-closed on
   hostile fragments: byte caps on text, ids, names, and arguments, a
   tool-call count cap, duplicate-index and invalid-JSON rejection. An
   event after the terminal is an error; a stream that never reaches one
   is `Unfinished`. A repeated terminal with the same stop reason is an
   accounting frame — its usage merges and it never becomes a second
   terminal.
5. The `craft-runtime` runner turns each `FoldedTurn` into either the
   next request — tool calls executed through the `Registry` — or the
   terminal `RunOutcome`, appending every model-visible message to the
   session record along the way.
6. `craft-app` prints the NDJSON event stream and exits with the
   outcome's code.

## Never-crash posture

craft-agent runs headless inside CI: a malformed input, a hostile file,
an oversized field, or a dead network must produce a typed error the
caller can act on — never a panic, never a lost run. The posture is
enforced, not aspirational:

- Workspace lints (`[workspace.lints]`): `unsafe_code = "forbid"`;
  `unwrap_used`, `expect_used`, `panic`, `unimplemented`, `todo`, and
  `unreachable` all `deny` — panicking shortcuts are compile errors
  outside tests (`clippy.toml` allows them in tests, so a failing test
  still fails loudly).
- Every fallible step returns `Result<T, E>` and propagates with `?`;
  context is added (`map_err`, error variants), never discarded.
- Every input-derived number is hostile — model output, tool results,
  SSE bytes, CLI parameters, file contents. Index and slice with
  `get()`, cut strings only at char boundaries, use
  `checked_*`/`saturating_*`, never divide by a value that can be zero,
  and cap allocations sized by input.
- Nothing iterates on model output without a bound: turn caps, tool
  limits, budgets, and the stationarity guard bound every loop the
  model can drive.
- Every crash fix ships with a small synthetic regression test that
  panicked before the fix.

## Exit codes

The process speaks a five-value taxonomy (`craft-runtime` `ExitStatus`):

| Code | Status | Meaning |
|---|---|---|
| 0 | `Ok` | The run completed; the terminal stream-json event carries the one-line outcome detail. |
| 1 | `Error` | Any failure the run could not convert into motion: provider, session, or configuration failures. |
| 2 | `MaxTurns` or `Stationarity` | The motion budget ran out — the turn cap was reached, or the stationarity guard stopped a run that stopped making progress. The cancellation category distinguishes the two. |
| 3 | `Budget` | The token budget failed closed: exhausted, or unverifiable because the provider reported incomplete usage. |
| 4 | `Denied` | A tool call touched a sensitive path or escaped the workspace root; the run aborts and the denial names the path. |

The terminal `RunOutcome` carries the exit status, a cancellation
category, a one-line actionable detail, usage accounting, the turn
count, and the session path. The app layer prints it and exits with
`exit_code()`.

## Command surface

The binary accepts two typed commands. `run` drives one prompt through
the loop: `--prompt` names a file or `-` for stdin, `--output-format`
selects `stream-json` (the machine contract, the default) or `plain`
(the human fallback), and the turn cap, token budget, and idle timeout
are explicit arguments. `digest` builds the suite digest over a bounded
look-back window. A run failure prints one actionable line and exits 1;
a malformed argument exits 2 with the argument parser's usage text.
The agent is headless: no TUI, no plugins — one process, NDJSON output,
the exit-code table above.

The wire dialects are documented in `02-providers.md`; the tool
contract, the loop, and the session in `03-tools-and-loop.md`.
