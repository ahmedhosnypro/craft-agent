# 06 — Testing

Two corpus tiers:

- **Keyless corpora** always run in every gate and in CI: recorded
  request/SSE fixtures per adapter (request encoding, SSE decode,
  tool-call mapping, finish reasons, usage accounting, hostile inputs) and
  the runtime/tool corpora (progress-to-terminal semantics,
  sensitive-file guards, timeout and kill sequencing, budget stops). Every
  feature lands with its corpus authored RED first.
- **Live corpora** talk to a real endpoint via the repo-root `.env`
  (`TEST_BYOK_*` knobs — see `.env.example`); every live test is
  `#[ignore]`d and runs through its `just integration*` recipe. Without
  `.env` each one prints its SKIP reason and passes — never fails — so CI
  stays green.

The gate: `just check` (fmt, clippy, tests, secrets scan, identifier
scan), mirrored exactly by CI. Both scanners carry `--selftest` canary
modes and fail closed on a broken scan.
