# 06 — Testing

Two corpus tiers, one gate. Keyless fixture corpora run in every gate and
in every CI run — no credentials, no network, deterministic. Live corpora
talk to a real endpoint through the repo-root `.env` and SKIP — never
fail — when that file is absent, so CI stays green without credentials and
the same corpora pass for real wherever a `.env` exists. Tests are authored
RED first: a feature without its corpus is not a feature.

## Keyless corpora

`cargo test --workspace` runs the whole keyless fixture corpus:

| Crate | Surface |
|-------|---------|
| `craft-core` | the neutral vocabulary: request/event types, SSE, retry, usage |
| `craft-providers` | the three protocol adapters |
| `craft-tools` | the Tool trait and the loop's builtins |
| `craft-runtime` | the run loop, budget caps, stream-json output |
| `craft-app` | the CLI surface |

Four shapes recur across every corpus:

- **Round-trips** — what crosses a boundary survives unchanged: neutral
  request/event types serialize and deserialize (`types_roundtrip`), and
  each adapter encodes a request and folds its decoded SSE stream back
  into the same neutral events.
- **Folds** — a stream of provider events folds into one complete turn
  with usage accounting and a stop reason; fold caps make oversized
  fields and runaway tool-call counts fail closed.
- **Hostile rows** — every input-derived value is treated as hostile:
  overflowing `max_tokens`, malformed retry-after headers, hostile glob
  patterns and tool arguments, hostile markers in machine-readable
  input. Each must land as a typed error or a bounded fallback — the
  synthetic regression corpus behind the no-panic rule.
- **Loopback mocks** — the adapter fixture corpora serve their recorded
  request/response rows off a loopback HTTP lane, so request encoding,
  SSE decode, the error taxonomy, and truncation bounds are exercised
  against a real socket — no credentials, no network.

## Live corpora

Seven `#[ignore]`d corpora ride a real endpoint through the repo-root
gitignored `.env`; none is part of `just check`. Six ride the
OpenAI-compatible seam; the Anthropic corpus rides the Messages seam
through its own knob trio; Gemini is fixtures-only in v1.

- `just integration` — completions round-trip
- `just integration_tools` — tool round-trip
- `just integration_stream_tools` — streamed tool-delta assembly
- `just integration_auth` — a bad key lands a typed error
- `just integration_dialect_mount` — dialect mount over one base URL
- `just integration_anthropic` — Anthropic Messages: streamed turn,
  forced tool call, bad-key auth
- `just integration_agent_loop` — the full agent loop: real turn, tool
  dispatch, terminal record

The knobs are a trio — `TEST_BYOK_BASE_URL`, `TEST_BYOK_API_KEY`,
`TEST_BYOK_MODEL` — loaded by `craft-providers/tests/byok_env.rs`, which
every corpus binary mounts via `#[path]`: process env wins, then a walk
up from the crate manifest to the repo `.env`. The Anthropic corpus
carries its own trio through the same loader —
`TEST_BYOK_ANTHROPIC_BASE_URL`, `TEST_BYOK_ANTHROPIC_API_KEY`,
`TEST_BYOK_ANTHROPIC_MODEL` — and its SKIP line names the first missing
knob. Names are printable; values never are.

The skip idiom, verbatim from `byok_env.rs` — the test mounts the loader
and asks for the trio; missing keys print the SKIP line and pass:

```rust
#[tokio::test]
#[ignore = "live corpus: requires TEST_BYOK_* in .env"]
async fn integration() -> Result<(), TestError> {
    let keys = crate::require_keys!("completions e2e");

// macro_rules! require_keys — the SKIP arm:
eprintln!(
    "SKIP: TEST_BYOK_* are not set (no .env) — the live {} corpus is off",
    $corpus
);
```

**SKIP, never fail.** A missing or empty knob prints its SKIP reason and
passes, so CI — which carries no `.env` — always runs every live corpus
and stays green. A knob that is set but broken (bad key, dead gateway)
is a loud FAIL, never a SKIP; `integration_auth` exists to prove the
bad-key path fails with a typed error.

## The gate

`just check` before every commit — nothing is done until it is green.
Thirteen stages, in order:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace` — the keyless corpus above
4. `./scripts/scan-secrets.sh`
5. `./scripts/scan-secrets.sh --selftest`
6. `./scripts/identifier-scan.sh`
7. `./scripts/identifier-scan.sh --selftest`
8. `./scripts/check-file-length.sh`
9. `./scripts/validate-agents.sh`
10. `./scripts/validate-agents.sh --selftest`
11. `cargo deny --locked check`
12. `cargo machete`
13. `cargo build --locked`

What the non-compiler stages enforce:

- **Secrets scan** — credential patterns plus the tracked-`.env` rule.
- **Identifier scan** — the no-lineage gate over the whole tracked tree.
  All three scanners fail closed — a scan that cannot run is a red gate,
  never a silent pass — and each `--selftest` proves the canary in both
  directions: every banned pattern or lint rule fires, and legal content
  never trips.
- **File length** — ≤300 effective lines per Rust file, tests included,
  no exemptions; functions stay under 75 (`clippy.toml`).
- **Agent validation** — `.craft-agent/agents/*.md` frontmatter, identity,
  body, and tools/model shape rules; a missing or empty agents dir is a
  printed skip, never a failure.
- **`cargo deny`** — licences, advisories, sources.
- **`cargo machete`** — zero unused dependencies.
- **`cargo build --locked`** — the lockfile is the contract.

## CI mirrors everything

`.github/workflows/ci.yml` runs the thirteen gate stages verbatim — hosted
runners lack `just`, so the workflow spells each command out, and every
gate change lands in the justfile and the workflow in the same commit
(the lockstep law). CI then runs the seven live corpora as plain
`--ignored` test invocations: the runner carries no `.env`, so each
prints its SKIP line and passes, and the same corpora pass for real
wherever a `.env` exists.
