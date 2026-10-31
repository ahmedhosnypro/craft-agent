# 05 — The GitHub Action

`action.yml` is a composite action: it downloads the release binaries for
the runner triple (Linux x86_64 and aarch64, statically linked — the
agent plus its `craft-helpers` companion), sha256-verifies them against
the `checksums.txt` from the same release
tag, and runs one mission — `run` or `digest`. The digest workflow and
the live-integration workflow are the two shipped consumers.

## Inputs

| Input | Default | Purpose |
|---|---|---|
| `mission` | `run` | The craft-agent command the action runs: `run` or `digest`. The CLI's third command, `workflow`, is not selectable through the action. |
| `provider` | required | `anthropic`, `openai`, or `gemini`. |
| `model` | required | The model id requested from the provider. |
| `args` | empty | Extra arguments for the mission command, space separated (for example `--org storytold --since 7d`). |
| `max-turns` | `40` | Agent turn cap. |
| `max-tokens` | `4000000` | Total token budget cap. |
| `prompt` | empty | Prompt text for the `run` mission; written to a file before the run. |
| `github-token` | `github.token` | Exported as `GH_TOKEN` for the agent's gh tool. Also installed as the runner-wide git credential helper unless `write-repo` is set — when the write gate is declared, the helper is skipped so a plain `git push` cannot go around it. |
| `step-retries` | `3` | Total attempts per model step when the gateway drops a stream mid-turn; `0` restores fail-fast. |
| `result-window-turns` | `0` | Turns of tool results kept verbatim in the assembled prompt; older results ride as size stubs. `0` (default) disables trimming, preserving gateway cache affinity. |
| `max-context-tokens` | empty | Context window cap in tokens; enables auto-compaction when set. Empty keeps it off. Forwarded as `CRAFT_AGENT_MAX_CONTEXT_TOKENS` and as `--max-context-tokens` when non-empty. |
| `max-input-tokens` | empty | Input-side compaction trigger limit in tokens; empty falls back to the context cap. Forwarded as `CRAFT_AGENT_MAX_INPUT_TOKENS` and as `--max-input-tokens` when non-empty. |
| `max-output-tokens` | empty | Per-request output token cap; empty keeps the default 8192. Forwarded as `CRAFT_AGENT_MAX_OUTPUT_TOKENS` and as `--max-output-tokens` when non-empty. |
| `thinking-level` | empty | Thinking level string; `none`/`off` disables thinking, anything else passes through to the provider verbatim. Empty keeps the provider default. Forwarded as `CRAFT_AGENT_THINKING_LEVEL` and as `--thinking-level` when non-empty. |
| `egress-allowlist` | empty | Comma-separated hosts bash commands may talk to; empty means no egress gate. |
| `write-repo` | empty | `owner/name` the gh tool's write verbs must target; empty means no write gate. Setting it also skips `gh auth setup-git`, since the credential helper would bypass the gate for plain git. |
| `read-roots` | empty | Extra read-only roots for `read_file`/`glob`/`grep`, comma separated, absolute paths; empty defaults to the runner temp directory, `off` disables the feature. |
| `version` | `v0.0.5` | Release tag the binary is downloaded from. |
| `release-repository` | `ahmedhosnypro/craft-agent` | Repository hosting the release assets. |

Provider keys are **not** inputs. They reach the binary only through the
environment of the step that uses the action — map
`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GOOGLE_API_KEY` from secrets
in the consuming workflow's step `env`. The binary reads the one matching
`provider` and removes provider-key variables from every child process.

No input is ever interpolated into a run script. Every input is mapped to
an environment variable and read from there, so untrusted values cannot
become shell syntax; the `args` string is word-split without globbing or
re-parsing.

## Outputs

| Output | Meaning |
|---|---|
| `conclusion` | `success`, `error`, `max-turns`, or `budget` — the exit-code taxonomy. |
| `exit-code` | The mission command's typed exit code (0, 2, 3, 4, or an unmapped failure code); `conclusion` is its named form. |
| `final-message` | The terminal detail from the `run_finished` stream-json event. |
| `digest-path` | The digest markdown path; set for the digest mission on success only. |

The stream-json transcript lands in `$RUNNER_TEMP/craft-agent-stream.jsonl`
and its last events are echoed into the step log when the exit code is
nonzero.

## Binary download and integrity

Each release attaches `craft-agent-x86_64-unknown-linux-musl`,
`craft-agent-aarch64-unknown-linux-musl`,
`craft-helpers-x86_64-unknown-linux-musl`,
`craft-helpers-aarch64-unknown-linux-musl`, and `checksums.txt` at the
same tag. The action picks the assets for `${RUNNER_OS}/${RUNNER_ARCH}`
— any other triple fails with an actionable error — downloads
`checksums.txt` first, downloads the recorded binaries under the exact
names the checksums
expect, and runs `sha256sum --check`. This pins integrity within a
release; it is not a supply-chain attestation. An unsupported runner
never falls back to building from source.

## The digest workflow

`.github/workflows/digest.yml` runs daily (`17 6 * * *`) and on manual
dispatch, with a `suite-digest` concurrency group that queues runs
instead of piling or cancelling them (a run superseded while queued is
cancelled by the queue and intentionally leaves no trace). The top-level
permission is `contents: read`. The sticky issue lives in this
repository while the digested suite is the `storytold` organization, a
different owner — no single token spans both. There are two jobs, and
each mints its own GitHub App token — a token is revoked in its minting
job's post-step, so the split is also a credential boundary:

- **digest** (45 min) — mints `contents: read` + `issues: read`, scoped
  to the repositories of `config/digest-repos.txt` (built at run time,
  so the curated list stays the single source of truth); checks out
  this repo (depth 1); derives the window by reading the sticky issue
  in this repository on the workflow's own read-only token; installs
  the pinned copy-paste detector; runs the action with the provider
  key in step env; uploads the digest as the `digest` artifact.
- **publish** (5 min) — mints `issues: write` scoped to this repository
  only; downloads the artifact; locates the sticky issue by exact title
  (`Suite digest`, authored by `app/craft-agent`, newest match wins,
  closed matches are updated and reopened); on success replaces the
  issue body; on a failed, cancelled, or artifact-less digest run it
  appends a dated failure-note **comment** and never touches the body or
  its window marker. A red run is never silent.

The window: the digest job derives a window start from the sticky
issue's `digest-window` marker — the marker's end date, clamped to 30
days, with a 24-hour fallback when the marker is missing or malformed —
and forwards it to the action's `args`. The binary parses `--since` as
a relative look-back (`<n><s|m|h|d>`); a manual dispatch can pass one
explicitly to bypass the marker, and `dry-run` publishes nothing at
all.

## The schedule guard

Scheduled runs proceed only when the repository variable
`DIGEST_ENABLED` is `"true"`; `workflow_dispatch` always runs. The
workflow ships before the App and its secrets exist, and the guard keeps
the dormant tree from firing broken daily runs in the meantime. Both
jobs carry the guard, so a guarded-out schedule never reaches publish
either. The enable step is part of the pilot checklist
(`docs/runbook-secrets.md`).

## The live-integration workflow

`.github/workflows/integration-live.yml` is dispatch-only — never
`pull_request`, never `pull_request_target` — so the gateway key never
enters a fork-controlled context. An empty secret is a hard failure with
a `::error::` naming the secret to configure; locally the same corpora
SKIP when `.env` is absent. Secrets are step-scoped and mapped to the
`TEST_BYOK_*` names; the base URL comes from a secret, never from
repository content. Every recipe mirrors the justfile verbatim.

## Pinning guidance

Every third-party action referenced anywhere is pinned by full commit
SHA with the version in a trailing comment. Consumers of this action
should do the same: pin `ahmedhosnypro/craft-agent` by full commit SHA and
pin the `version` input to an exact release tag for reproducibility. The
in-repo digest workflow deliberately consumes `./` with
`version: v0.0.5`, pinning the daily digest to an exact release.
