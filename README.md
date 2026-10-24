# craft-agent

Available on the [GitHub Marketplace](https://github.com/marketplace/actions/craft-agent).

BYOK agentic AI agent for GitHub Actions. The action downloads the
release binaries for the runner, verifies them against the release
checksums, then runs one bounded mission: an agent run over a prompt,
or a digest over a window of merged pull requests. Bring your own
provider key — no key is an action input, and the action stores no
secret of its own.

The agent is a single pure-Rust binary that drives a prompt through a
model-and-tool loop under explicit caps: turns, tokens, output sizes,
time. Seven built-in tools — `bash`, `read_file`, `write_file`,
`edit_file`, `glob`, `grep`, `gh` — operate inside a confined
workspace, and every failure is a typed exit code the calling workflow
can branch on. Never a panic, never a lost run.

## Usage

```yaml
permissions:
  contents: write
  pull-requests: write

jobs:
  triage:
    runs-on: ubuntu-latest
    steps:
      - name: Check out the repository
        uses: actions/checkout@v7
      - name: Run the agent
        uses: ahmedhosnypro/craft-agent@v0.0.12
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        with:
          provider: openai
          model: ${{ secrets.MODEL_ID }}
          prompt: >-
            Look at the most recent failed CI run, diagnose the most
            likely cause, and open a pull request with the fix.
          egress-allowlist: github.com,api.github.com
          write-repo: ${{ github.repository }}
```

Supported runners are GitHub-hosted Linux x86_64 and aarch64; any
other triple fails with an actionable error and never falls back to
building from source. Pin the action by release tag (`@v0.0.12`) or by
full commit SHA (frozen); the `version` input selects the release tag
the binaries download from.

## Bring your own key

The action defines no secrets and accepts no key as an input. Map the
selected provider's key into the step's `env`:

| `provider` | Required env | Optional env |
|---|---|---|
| `openai` | `OPENAI_API_KEY` | `OPENAI_BASE_URL` — any OpenAI-compatible endpoint |
| `anthropic` | `ANTHROPIC_API_KEY` | `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH` (`x-api-key` or `bearer`) |
| `gemini` | `GOOGLE_API_KEY` | — |

The binary reads only the key matching `provider`, and provider-key
variables are removed from every tool child process — a `bash` or `gh`
call never inherits them.

## Inputs

| Input | Default | Purpose |
|---|---|---|
| `mission` | `run` | The command to run — `run` or `digest`. |
| `provider` | required | Provider id — `anthropic`, `openai`, or `gemini`. |
| `model` | required | Model id requested from the provider. |
| `prompt` | empty | Prompt text for the `run` mission; written to a file before the run. |
| `args` | empty | Extra arguments for the mission command, space separated. |
| `max-turns` | `40` | Agent turn cap. |
| `max-tokens` | `4000000` | Total token budget cap. |
| `github-token` | `github.token` | Token the gh tool uses, exported as `GH_TOKEN`. |
| `step-retries` | `3` | Total attempts per model step when the provider drops a stream mid-turn; `0` restores fail-fast. |
| `result-window-turns` | `0` | Turns of tool results kept verbatim in the assembled prompt; older results ride as size stubs. `0` disables trimming. |
| `egress-allowlist` | empty | Comma-separated hosts bash commands may talk to; empty means no egress gate. |
| `write-repo` | empty | `owner/name` the gh tool's write verbs must target; empty means no write gate. |
| `read-roots` | empty | Extra read-only roots for `read_file`/`glob`/`grep`, comma separated, absolute paths; empty defaults to the runner temp directory, `off` disables the feature. |
| `version` | `v0.0.12` | Release tag the binaries are downloaded from. |
| `release-repository` | `ahmedhosnypro/craft-agent` | Repository that hosts the release assets. |

No input is ever interpolated into a run script: every input reaches
the binary through the environment, so an input value can never become
shell syntax.

## Outputs

| Output | Meaning |
|---|---|
| `conclusion` | `success`, `error`, `max-turns`, or `budget`. |
| `exit-code` | The mission's typed exit code; `conclusion` is its named form. |
| `final-message` | Terminal detail from the `run_finished` stream-json event. |
| `digest-path` | Digest markdown path; set for the digest mission on success only. |

The run's NDJSON transcript lands in
`$RUNNER_TEMP/craft-agent-stream.jsonl`; the last events are echoed
into the step log on a nonzero exit.

## Exit codes

| Code | `conclusion` | Meaning |
|---|---|---|
| 0 | `success` | The run completed. |
| 1 | `error` | Provider, configuration, or session failure. |
| 2 | `max-turns` | The turn cap was reached, or the stationarity guard stopped a run that stopped making progress. |
| 3 | `budget` | The token budget ran out, or usage could not be verified. |

## Run summaries

On GitHub Actions the binary writes its own run-page summary: when
`GITHUB_STEP_SUMMARY` is set (the runner sets it for every step), each
mission appends a markdown record to that file — conclusion and exit
code, model, turns of the cap, token spend of the budget, the
deliverables the run surfaced (pull-request links and pushed branches),
the final message, and a transcript tail — and prints one
`::notice::` annotation into the step log. Unset locally, it is a
byte-identical no-op. Consumers need no workflow edits: the binary
detects the env, and the summary is best-effort — a summary that
cannot be written is one warning, never a failed run.

## Security

**Caller's token, caller's permissions.** `github-token` defaults to
`github.token` and reaches only the agent's gh tool; what it can do
is exactly what the calling job's `permissions` block grants.
Read-only missions need `contents: read`; a run that opens pull
requests needs `contents: write` and `pull-requests: write`. The
action defines no secrets of its own — keys arrive per run, through
the caller's step environment.

**Workspace confinement.** File tools resolve every model-supplied
path against the workspace root and reject escapes twice: lexically
(a `..` that would leave the root), and through the filesystem (the
deepest existing ancestor must stay inside the root, so symlink
escapes are caught). Reads may extend into the `read-roots` roots;
writes stay workspace-only.

**Sensitive paths are refused, never fatal.** Cloud credential
directories, `.env*` files (except the `.env.example`, `.env.sample`, and
`.env.template` spellings), private-key and token files, and
`credentials*` files are never opened; the refusal returns to the model
as an error result naming the path, and the run continues. A model that
hammers on refusals runs into the turn cap or the stationarity guard.

**Egress allowlist.** With `egress-allowlist` set, bash commands may
talk only to the listed hosts; a command targeting another host is a
typed error. Empty leaves bash ungated.

**Write gate.** With `write-repo` set, the gh tool's write verbs must
target that one repository.

**Checksum-verified binaries.** The install step downloads
`checksums.txt` and the runner's two binaries — the agent and its
`craft-helpers` companion — from the same release tag, and fails
unless `sha256sum --check` passes. That is same-origin integrity
against corruption and mismatched pairs, not a supply-chain
attestation; the trust anchor is the ref you pin. Every release is
reproducible locally with `scripts/build-release.sh`.

**Secret redaction at the seam.** Every byte of tool output — progress
chunks, terminal results, tool errors — passes a redactor before it
reaches the model, the transcript, or any log. Known secret values are
collected from the step environment at startup and removed in every
encoding the wild has shown them in: raw, percent-encoded, hex, and
base64 of the value and its common header composites
(`x-access-token:`, `Authorization: Basic`, `Bearer`, …). A second
layer removes well-known token shapes (GitHub, OpenAI, Anthropic,
AWS, GCP, Slack, npm, JWTs, PEM key headers, URL credentials) with
rule ids carried from the open-source scanners they were ported
from. Run summaries show a `redactions` row with the counters; under
Actions each registered value is also `::add-mask::`-ed so the
rendered job log masks it too. `CRAFT_REDACT=off` disables the layer
for local runs (never do this in CI), and `CRAFT_REDACT_EXTRA` adds
values the environment scan cannot see. Redaction is not a
substitute for rotation: a value that reached a public artifact must
still be revoked.

**No panicking shortcuts.** The workspace forbids `unsafe` and
compiles with the panicking lints denied outside tests; every
fallible step is a typed error the caller can act on.

## Agent profiles

A run can adopt a named profile from `.craft-agent/agents/`: a
markdown file with flat YAML frontmatter — `name`, `description`,
`promptMode`, a `tools` allowlist with `disallowedTools`, `maxTurns`
(a fallback when the turn cap is unset), `timeoutSecs` — and a body
that becomes the profile's prompt. The registry enforces the tool
allowlist, `promptMode` decides whether the body replaces or extends
the default system prompt, and `timeoutSecs` caps the run's wall
clock.

`--agent <name>` resolves from the closest `.craft-agent/agents`
directory, walking up from the working directory to the git root — a
repository ships its own profiles next to the code they describe. The
action reaches the flag through the `args` input:

```yaml
with:
  args: "--agent repo-scout"
```

`scripts/validate-agents.sh` lints every profile — frontmatter keys,
identity rules, bounded tool and model values — and runs as part of
the default gate.

## The digest mission

`mission: digest` renders a markdown digest over a bounded look-back
window of merged pull requests: per-repository summaries, cross-repo
patterns, and duplicate-code candidates, each claim linked to its
source. The window rides through `args` (`--since 7d`), the scope
through `--org <org>` or `--repos a,b`, and `--dry-run` renders to
standard output without writing the file. A successful run leaves
the digest at `out/digest.md` and reports it through the
`digest-path` output.

## Rehearsing a mission locally

`sim/` is a container rehearsal of the action: `bash sim/run-sim.sh
[task]` builds the release, then runs one mission inside a container
that behaves like a GitHub Actions runner. Each run leaves a
directory carrying a provenance manifest, the container log, the
action-shaped outputs, the stream transcript, and the workspace the
mission ran against.

A judged workflow rehearses the same way:

    CRAFT_AGENT_MISSION=workflow CRAFT_AGENT_WORKFLOW=<plan> \
      bash sim/run-sim.sh

The container runs `craft-agent workflow` against a fresh clone with
the action's knobs, mirrors the run journal and per-step sessions
into `out/`, and `sim/assert-workflow.sh sim/runs/<run>` checks the
finished run against the proofs the fix must show — spawn tools live,
parallelism reached, gates settled, wall time under a ceiling — before
the workflow ever reaches CI. See `sim/README.md`.

The `simulate` mission is the cheap gate: it drives the judged
workflow's full 16-step architecture — fan-out waves, panel gates, a
repaired seat, appeals, committers — over a scripted model with local
fixtures, so it needs no gateway, no token and no network and settles
in seconds:

    CRAFT_AGENT_MISSION=simulate bash sim/run-sim.sh

That is the first thing a workflow-mode CI rehearsal runs: the engine's
wiring is proven before any real gateway spend.

## Contributing

`just check` is the full gate, mirrored exactly by CI: formatting,
clippy with `-D warnings`, workspace tests, the secret and identifier
scans (each with a `--selftest` canary), file-length limits,
agent-file validation, `cargo deny`, `cargo machete`, and
`cargo build --locked`. Commits follow the conventional-commit style.

`docs/` covers the architecture, the provider adapters, the tools
and the run loop, the digest, the action contract, and testing in
depth. Dual-licensed under MIT or Apache-2.0.
