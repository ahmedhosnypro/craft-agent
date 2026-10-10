# craft-agent — rules for agents and contributors

These rules apply to every human and AI contributor. They bind every task;
when a rule here conflicts with convenience, the rule wins.

## 0. Never crash

craft-agent runs headless inside CI: a malformed input, a hostile file, an
oversized field, or a dead network must produce a typed error the caller
can act on — never a panic, never a lost run. This rule outranks feature
work: do not ship a feature by adding a panic path.

1. **Fail with `Result<T, E>`.** Return the crate's error type and
   propagate with `?`; add context (`map_err`, an error variant) instead of
   discarding it. Exit codes are 0/1; every failure prints one actionable
   line.
2. **No panicking shortcuts outside tests:** no `unwrap()`, `expect()`,
   `panic!`, `unreachable!`, `todo()` or `unimplemented!`. Use `?`,
   `ok_or(..)?`, `let Some(x) = .. else { return Err(..) }`, `if let`, or a
   fallback that is truly correct. An unfinished feature returns an
   "unsupported" error. Enforced workspace-wide via `[workspace.lints]`;
   `clippy.toml` allows them in tests (a failing test should fail loudly).
3. **No `unsafe`** (`unsafe_code = "forbid"` for the workspace).
4. **Every input-derived number is hostile.** Model output, tool results,
   SSE bytes, CLI parameters, and file contents are untrusted: index and
   slice with `get()`, slice strings only at char boundaries, use
   `checked_*` / `saturating_*`, never divide by a value that can be zero,
   cap allocations sized by input.
5. **Bound recursion and loops.** Agent turns, tool output, and diff walks
   carry explicit caps; nothing iterates on model output without a bound.
6. **Prove it.** Every crash fix comes with a small synthetic regression
   test that panicked before the fix.

## 1. Clean-room code

- Never read or copy third-party implementation sources while implementing
  a protocol: the three provider protocols are implemented from official
  public documentation only.
- Dependencies must use permissive licences: MIT, Apache-2.0, BSD, ISC,
  Zlib, Unicode, CC0, or BSL-1.0; MPL-2.0 only unmodified.

## 2. Everything is a command

The CLI is typed commands with documented arguments (`craft-agent run`,
`craft-agent digest`), and the agent is itself agent-drivable:
`run --output-format stream-json` emits a documented NDJSON contract so
other agents can consume a run.

## 3. Gates before every commit

`just check`, and nothing is done until it is green:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `./scripts/scan-secrets.sh` (secret patterns + the tracked-`.env` rule)
- `./scripts/identifier-scan.sh` (the no-lineage gate)

`just check` is mirrored EXACTLY by `.github/workflows/ci.yml` (lockstep
law): every gate change lands in both in the same commit. Both scanners
fail closed — a scan that cannot run is a red gate, never a silent pass —
and both carry a `--selftest` canary mode proving they catch what they
claim. Files stay under 300 effective lines and functions under 75
(`clippy.toml`).

## 4. Comments: terse and timeless

Comments say what the code does at that line — plain, terse, timeless. No
dates, no incident history, no measurements, no task or spec IDs, no
markdown-file references, no change rationale (that is commit-message
material). Match the length and tone of the comments around the code you
are touching.

## 5. No lineage

The product never mentions the agent harnesses or reference actions it was
studied against — in code, comments, docs, commit messages, or anywhere
else. The enforced identifier and host-literal list lives in
`scripts/identifier-scan.sh`; the scan fails the build on any hit and covers
the whole tracked tree. The research trail (`analysis/`, `PLAN.md`,
`progress.db`, `scripts/progress-db.sql`) is the local-only record: those
paths are gitignored and never leave this machine, so the repository carries
no lineage traces at all.

## 6. Live corpora skip, never fail

Live integration tests read the repo-root `.env` (`TEST_BYOK_*` knobs —
see `.env.example`); every live test is `#[ignore]`d and runs through its
`just integration*` recipe. When any knob is missing or empty the test
prints its SKIP reason and passes — CI always runs these corpora, never
hides them. Key names are printable; values never are.

## 7. Working rules for agents

- One task = one tracked row in `progress.db` (`scripts/progress-db.sql`
  bootstraps it): mark `in_progress` when starting, `done` when finished,
  and record findings as you go.
- File writes are chunked: at most 50 lines per write call.
- Conventional commits; stage explicit paths only — never `git add -A`.
- Never read a live `.env` anywhere; the example/sample/template forms are
  the only env files an agent may open.
