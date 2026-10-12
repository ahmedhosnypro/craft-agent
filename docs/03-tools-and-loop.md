# 03 — Tools and the run loop

Tools are typed streams; the loop is turn-bounded. The session record
is both the wire transcript and the log — everything model-visible is
logged. The one exception runs one way: result-window trimming stubs
old tool results out of the assembled prompt while the record keeps
them verbatim.

## The Tool trait

A tool is a name, a description, a JSON Schema for its parameters, and
one executor: `execute(args, ctx) -> ToolStream`. `execute` itself
never fails; every failure arrives as a typed error inside the returned
stream, so no tool call can die on a panic path and no caller has to
guess whether a tool ran.

## The stream contract

A `ToolStream` is any number of `Progress` events — partial output,
tagged stdout or stderr — followed by exactly one terminal item: a
`Terminal` result or a typed `ToolError`. The registry wraps every
stream with an enforcer that turns violations into typed errors:
`MissingTerminal` for a stream that ends without a terminal,
`DoubleTerminal` for a second one, and `TrailingProgress` for progress
after the terminal. An unknown tool name surfaces the same way, as an
`UnknownTool` error inside the stream.

## The Registry

Tools are name-keyed. A tool's wire-facing declaration — name,
description, parameter schema — is computed once at registration and
cached, so the model sees a stable tool list and request assembly
reads the cache. `execute(name, args, ctx)` dispatches with the stream
contract enforced; the run mission mounts the seven builtins through
`Registry::with_builtins`, or a filtered subset when `--agent`
selects a profile whose `tools` allowlist (minus `disallowedTools`)
narrows the set.

## ToolCtx and ToolLimits

`ToolCtx` is the per-run context every tool receives: the workspace
root, the child-environment source (the real process environment in
production, an explicit map in the corpora), and the read stamps
backing `edit_file`'s prior-read and changed-on-disk guards. The
scrubbed child environment is computed per call, never cached.
`ToolLimits` bounds every builtin; defaults:

| Limit | Default | Bounds |
|---|---|---|
| `bash_default_timeout` | 120 s | A bash call without an explicit timeout. |
| `bash_max_timeout` | 600 s | Ceiling for an explicit bash timeout. |
| `proc_grace` | 10 s | SIGTERM-to-SIGKILL grace window for tool children. |
| `proc_out_cap` | 256 KiB | Combined child process output. |
| `gh_timeout` | 60 s | One gh invocation. |
| `read_cap` | 2 MiB | One `read_file`. |
| `write_cap` | 2 MiB | One `write_file` payload. |
| `max_path_bytes` | 4096 | Any tool-supplied path. |
| `max_command_bytes` | 64 KiB | A bash script. |
| `max_pattern_bytes` | 8 KiB | A glob or grep pattern. |
| `max_args` | 64 | Elements in a tool's list argument (gh args). |
| `glob_max_results` | 200 | glob matches reported. |
| `grep_max_results` | 200 | grep matches reported. |
| `walk_max_entries` | 20 000 | Directory entries per walk. |
| `walk_max_depth` | 32 | Directory walk depth. |
| `grep_file_cap` | 10 MiB | One file's grep input. |
| `grep_line_cap` | 500 | Characters reported for a grep match. |

## The builtins

| Tool | Purpose and bounds |
|---|---|
| `bash` | Runs a bash script in the workspace with closed stdin and a bounded timeout; child output streams as progress; nonzero exits and timeouts are typed errors; the kill sequence is SIGTERM → grace window → SIGKILL. |
| `read_file` | Reads a UTF-8 text file inside the workspace; content above the size guard is rejected, never truncated. |
| `write_file` | Writes a full file inside the workspace; missing parent directories are created; the payload is capped. |
| `edit_file` | Replaces an exact byte match in a previously read file; aborts if the file changed on disk; an ambiguous match is a typed error unless replace-all is requested. |
| `glob` | Lists workspace files matching a glob pattern, sorted, with a bounded result count. |
| `grep` | Searches file contents with a regex or literal pattern across the workspace, bounded and sorted. |
| `gh` | Runs a gh CLI command with `GH_TOKEN`; all GitHub I/O goes through this tool; `--json` output must parse, and a nonzero exit is a typed error. |

## Sensitive paths

File tools resolve every model-supplied path against the workspace
root and reject escapes twice: lexically (components fold, and a
`..` that would leave the root is a typed `EscapesWorkspace` error)
and through the filesystem (the deepest existing ancestor must
canonicalize inside the root, so symlink escapes are caught too).

The sensitive-path predicate is a pure function of the resolved path,
checked per call against every component — nothing is cached. It
denies: cloud credential directories (`.aws`, `.gcp`), environment
files (any `.env*` except the exact spellings `.env.example`,
`.env.sample`, `.env.template`), private key and token files (`id_rsa*`,
`id_dsa*`, `id_ed25519*`, `id_ecdsa*`, and any name ending `.pem`,
`.key`, `.netrc`, or `.npmrc`, unless it ends `.pub`), and
`credentials*` files.

A denial is run-fatal, not a tool result: `SensitivePath` and
`EscapesWorkspace` abort the run through the exit-4 family, while every
other tool error round-trips to the model as an `is_error` tool
result it can react to.

## Environment scrubbing

Tool children never see provider keys. The child environment is
filtered per call: `*_API_KEY` (and `API_KEY`) and the whole
`TEST_BYOK_*` family are removed; `GH_TOKEN` and everything unrelated
pass through in order.

## The loop

`Runner` drives one turn at a time to a terminal outcome. Knobs
(`RunConfig`): a system prompt, the turn cap (default 40), the total
token budget (default 4 000 000), the per-request output cap forwarded
to the provider (default 8192), turn caps for the fold, an optional
idle timeout over a stalled stream, the step-attempt bound (default 3,
env `CRAFT_AGENT_STEP_RETRIES`), the result-window turn count (default
20, env `CRAFT_AGENT_RESULT_WINDOW_TURNS`), the workspace root, and
the session path. Each turn:

1. The runner builds the next request from the full session record
   plus the registry's cached tool specs, and streams it through the
   provider, forwarding text deltas as events and folding the stream
   into one turn (the idle timeout bounds a stalled stream). A stream
   that dies mid-turn replays the step: the partial fold is discarded,
   the same request is resent up to the step-attempt bound, and every
   replay emits a `turn_retried` event. Failures that surfaced before
   the stream — the provider's connect ladder already retried them —
   and deterministic classes (auth, invalid request, context window,
   payload size, quota) never replay.
2. A folded turn with neither text nor tool calls replays the same
   way, unless the provider filtered it: a zero-content terminal stop
   is an error, never success.
3. Usage is summed into the run total and the assistant turn is
   appended to the session.
4. The budget is checked; a spend at or over the cap ends the run
   with exit 3 — every turn is charged, including the one that
   completes the mission, so a final answer cannot silently overshoot
   the cap.
5. A turn with no tool calls ends the run: completed, exit 0.
   Otherwise the tool calls execute in order — one `tool_call` event,
   one executed call, one appended `Tool` message and `tool_result`
   event each — with the stationarity guard observing every executed
   call.

Every ending passes through one `finish` path, so the terminal
`run_finished` event and the process outcome can never disagree.

## Budgets

The budget is a single fail-closed token check between turns, before
any tool of the next round executes and before completion is decided.
The run total sums per-turn usage with saturating adds (repeats inside
one turn were already merged by the fold), and the `incomplete` flag
ORs across turns. Spent means input + output + cache-read +
cache-write + reasoning tokens — the parser emits disjoint buckets
that sum to the provider's totals, so the sum is honest and cached
prefixes still count as real context. Two
outcomes, both exit 3: the spend reached the budget, or the budget
cannot be verified because the provider reported incomplete usage — an
unverifiable budget is never treated as unspent. Replays keep their
usage charged: an empty-but-complete attempt adds its usage to the run
total before the step replays, and a replay the budget can no longer
fund ends the run with the same exit 3 instead of firing.

## Stationarity

The guard compares tool calls by a canonical signature: the tool name
plus its arguments with object keys sorted recursively, so equivalent
JSON that differs only in key order or spacing counts as the same
call; unparseable arguments fall back to their raw text. Any different
call resets the streak. On the 4th consecutive identical call the
runner injects a user nudge before the next request — change approach
or finish — once per streak; on the 8th the run stops: stationary,
exit 2, with the cancellation category distinguishing it from max-turns.

## Session and replay

The session is an append-only JSONL record: one JSON `ChatMessage` per
line, appended once and never rewritten. Opening a record loads it
verbatim, so a resumed run rebuilds the prompt from the full record
under the same result-window rule.
Loading is fail-closed: a blank line, an unparsable line, a line over
4 MiB, or a file over 64 MiB is a typed error naming the line. The
invariant is structural — the next request is the session's message
list with results older than the window stubbed, so nothing reaches
the model without being logged, and nothing is logged that did not
reach the model. A run without a
session path keeps the transcript in memory only.

## The NDJSON event stream

`craft-agent run --output-format stream-json` prints one JSON object
per line, tagged by `type`:

| Event | Fields |
|---|---|
| `turn_started` | `turn` |
| `assistant_delta` | `turn`, `text` |
| `turn_retried` | `turn`, `attempt`, `reason` |
| `tool_call` | `turn`, `index`, `id`, `name`, `arguments` |
| `tool_progress` | `turn`, `name`, `stream` (`stdout` or `stderr`), `text` |
| `tool_result` | `turn`, `call_id`, `name`, `is_error`, `content` |
| `turn_usage` | `turn`, `usage` |
| `run_finished` | `exit_code`, `category`, `detail`, `usage`, `turns` |

`run_finished` is always the last line; its `exit_code` doubles as the
process exit code, and `category` carries the cancellation taxonomy:
`completed`, `error`, `max_turns`, `stationarity`, `budget`,
`sensitive_path`, `escapes_workspace`. A `turn_retried` line marks a
replayed model step: the assistant text the run already streamed for
that turn since its previous boundary is void, `attempt` is the 1-based
number of the failed attempt, and `reason` is a stable token —
`stream_closed`, `transport`, `transient`, `timeout`, `idle_timeout`,
`rate_limit`, or `empty_response`.
