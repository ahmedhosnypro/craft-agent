# Runner simulation

`sim/` hosts a local rehearsal of the composite action: a container that
behaves like a GitHub Actions runner consuming the release binaries, so a
mission can be exercised end to end before it ships. `sim/entrypoint.sh`
mirrors the action's steps against runtime mounts instead of a release
download; `sim/run-sim.sh` is the host-side orchestrator.

## Running one mission

    bash sim/run-sim.sh [task]

Prerequisites: docker, host `gh`, and a repo-root `.env` carrying
`TEST_BYOK_API_KEY`, `TEST_BYOK_BASE_URL`, and a valid `GH_TOKEN` — the
same live-corpus knobs the integration tests use. Values are read at run
time and are never baked into the image or printed.

One run:

1. builds the release for `x86_64-unknown-linux-musl` (replacing `dist/`),
2. builds the `craft-agent-sim:sim` image on the host network,
3. clones a fresh workspace into the run dir,
4. runs one `run` mission in the container, preserving its exit code.

The image also takes direct binary invocations, e.g.
`docker run --rm craft-agent-sim:sim --version`.

## The run dir

Runs land in `sim/runs/<timestamp>/`:

| path | content |
|---|---|
| `manifest.json` | run provenance — see below |
| `container.log` | everything the container printed |
| `out/outputs.env` | the action-shaped outputs (below) |
| `out/stream.jsonl` | the stream-json transcript |
| `tmp/` | the mounted `RUNNER_TEMP`, holding the prompt file |
| `workspace/` | the clone the mission ran against |

The entrypoint chowns the mounted artifacts to the invoking user as it
runs, so a finished run is removable without sudo. Runs made before that
chown step may still hold container-owned files; reclaim one with
`sudo chown -R "$(id -u):$(id -g)" sim/runs/<run>`.

## The manifest

`manifest.json` is written host-side in three stages — at run-dir creation,
after the clone, and after the container exits — so a failed run still
describes itself. A failed clone records `status: "clone-failed"` and the
run dir is kept as evidence.

| field | value |
|---|---|
| `schema` | manifest shape version (`1`) |
| `status` | `cloning`, `clone-failed`, or `completed` |
| `task`, `mission` | the task file that ran; the mission command (`run`) |
| `repo`, `clone_commit` | cloned URL and the cloned commit SHA |
| `image`, `image_id` | the sim image tag and its docker digest |
| `binaries` | `sha256` of both release binaries, from `dist/checksums.txt` |
| `knobs` | the env overrides below — values only, never secrets |
| `started_at`, `finished_at`, `wall_seconds` | run timing |
| `exit_code`, `conclusion` | the container's exit code and its named form |

Keys and tokens (`GH_TOKEN`, provider keys) are never written to the
manifest, the log, or any artifact.

## Knobs

Environment overrides read by `run-sim.sh` at run time:

| env | default | meaning |
|---|---|---|
| `CRAFT_AGENT_MAX_TURNS` | `40` | turn cap passed to the mission |
| `CRAFT_AGENT_MAX_TOKENS` | `4000000` | token budget cap |
| `CRAFT_AGENT_PROVIDER` | `openai` | provider id |
| `CRAFT_AGENT_MODEL` | `glm-5.3-flash` | model id |
| `CRAFT_AGENT_STEP_RETRIES` | `3` | attempts per model step on a dropped stream |
| `CRAFT_AGENT_RESULT_WINDOW_TURNS` | `20` | turns of tool results kept verbatim in the prompt |
| `CRAFT_AGENT_RESULT_TRUNCATE_BYTES` | `4096` | bytes of each tool result kept verbatim in the prompt; oversized results ride as head plus a spill pointer |
| `CRAFT_AGENT_EGRESS_ALLOWLIST` | unset | hosts bash commands may talk to; unset or empty means ungated |
| `CRAFT_AGENT_WRITE_REPO` | unset | `owner/name` the gh tool may write to; unset or empty means ungated |
| `CRAFT_AGENT_READ_ROOTS` | `/runner/temp` | extra read-only roots for read_file/glob/grep, comma separated; the file tools read absolute paths inside them |

## Task files

`sim/tasks/<name>.md` is the prompt source; the first argument picks it
(default `open-pr-01`). The tasks directory is mounted read-only at
`/tasks` and the entrypoint passes `/tasks/<name>.md` as the prompt file —
the same seam the action's `prompt` input fills.

## Outputs

`out/outputs.env` mirrors the action's `GITHUB_OUTPUT` block:

- `conclusion` — `success`, `error`, `max-turns`, `budget`, or `denied`
- `exit-code` — the container's typed exit code
- `digest-path` — the digest markdown, digest mission on success only
- `stream` — the transcript path relative to the run dir
- `final-message` — the terminal `run_finished` detail, heredoc-quoted

Exit codes follow the documented taxonomy: 0 success, 2 max-turns,
3 budget, 4 denied-path; anything else maps to `error`, and a nonzero exit
prints the last stream events into the log. `run-sim.sh` prints the
conclusion, exit code, stream path, and final message when the run ends.
