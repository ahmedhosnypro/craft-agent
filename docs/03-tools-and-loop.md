# 03 — Tools and the run loop

Tools are typed streams; the loop is turn-bounded. The session record
is both the wire transcript and the log — everything model-visible is
logged. The exceptions run one way: result-window trimming stubs old
tool results and per-result truncation stubs oversized tails out of
the assembled prompt while the record keeps them verbatim.

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

## MCP client tools

Configured MCP servers extend the registry at run time. The config is
`.craft-agent/mcp.json` — the closest one from the workspace root, up
to the git root, with an optional host file at
`~/.craft-agent/mcp.json` merging first and the project file winning
per server name. Each entry names a stdio command with optional args,
an env overlay (provider keys never reach servers, not even through
the overlay), an in-workspace cwd, tool filters, and bounded startup
and per-call timeouts; the roster caps at 8 servers, the file at
64 KiB, and a url or non-`stdio` type is a typed v2-later error.

The whole surface is trust-gated by `CRAFT_AGENT_MCP`: unset, empty,
or `0` connects nothing (each configured server still reports a
`disabled` status event naming the knob), `1` is the workflow
author's explicit grant that the committed file's commands may run.
A config that cannot be honored fails the run closed; per-server
connect failures never do — they surface as `failed` status events
with the server's message and a 4 KiB stderr tail, and the run
proceeds without that server's tools.

Connected servers speak the newline-delimited JSON-RPC stdio
protocol: initialize handshake, `tools/list` to exhaustion, and
`tools/call` per invocation. Discovered tools register under
`mcp__<server>__<tool>` through the registry's run-time tool path —
same dispatch, same stream contract, same `UnknownTool` — with the
server's inputSchema passed through byte-identical after bounds:
object-shape check, 64 KiB per schema, 256 KiB per server,
composition depth 8. Caps: 64 tools per server, 128 across all
(`CRAFT_AGENT_MCP_MAX_TOOLS`), with a warning above 64. A name
collision drops the loser with a visible event, never a silent
overwrite. Tool output is untrusted: converted text caps at 256 KiB
with the truncation marker, unsupported media blocks drop with
notices, and a server that dies mid-run turns into a typed tool
error — one reconnect and retry, then the model sees the combined
failure and the run continues.

Two additive events ride the NDJSON stream, one status line per
configured server before the first turn and one discovery record
per connected server:

| Event | Fields |
|---|---|
| `mcp_server_status` | `name`, `status` (`connected`/`failed`/`disabled`), `transport`, `tool_count`, `error`, `note` |
| `mcp_tools_discovered` | `server`, `hash`, `tools`, `enabled_names` |
| `mcp_name_collision` | `server`, `tool`, `winner`, `reason` |
| `mcp_note` | `note` |

The discovery record's hash is a sha256 over key-sorted canonical
JSON of the record's own fields, so the raw definitions a server
handed over stay auditable in the stream record. Agent profiles
accept the tokens `mcp` and `mcp:<server>` in `tools` and
`disallowedTools` structurally; the runtime resolves them against
the discovered surface.

## ToolCtx and ToolLimits

`ToolCtx` is the per-run context every tool receives: the workspace
root, the child-environment source (the real process environment in
production, an explicit map in the corpora), the read stamps backing
`edit_file`'s prior-read and changed-on-disk guards, and the extra
read roots parsed once from `CRAFT_AGENT_READ_ROOTS` and fixed for the
run. The scrubbed child environment is computed per call, never cached.
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
| `read_file` | Reads a UTF-8 text file inside the workspace or a configured read root; content above the size guard is rejected, never truncated. |
| `write_file` | Writes a full file inside the workspace; missing parent directories are created; the payload is capped. |
| `edit_file` | Replaces an exact byte match in a previously read file; aborts if the file changed on disk; an ambiguous match is a typed error unless replace-all is requested. |
| `glob` | Lists workspace files matching a glob pattern, sorted, with a bounded result count; a configured read root is searchable too. |
| `grep` | Searches file contents with a regex or literal pattern across the workspace and any configured read roots, bounded and sorted. |
| `gh` | Runs a gh CLI command with `GH_TOKEN`; all GitHub I/O goes through this tool; `--json` output must parse, and a nonzero exit is a typed error. |

## Sensitive paths

File tools resolve every model-supplied path against the workspace
root and reject escapes twice: lexically (components fold, and a
`..` that would leave the root is a typed `EscapesWorkspace` error)
and through the filesystem (the deepest existing ancestor must
canonicalize inside the root, so symlink escapes are caught too).

The read tools (`read_file`, `glob`, `grep`) may also reach a root
named by `CRAFT_AGENT_READ_ROOTS`: absolute inputs try the workspace
first, then the declared roots in order, and a relative input never
leaves the workspace. The predicate and the symlink rules apply inside
those roots unchanged, each against its matched root — a symlink may
not stitch roots — and the write tools never see the knob:
`write_file` and `edit_file` stay workspace-only.

The sensitive-path predicate is a pure function of the resolved path,
checked per call against every component — nothing is cached. It
denies: cloud credential directories (`.aws`, `.gcp`), environment
files (any `.env*` except the exact spellings `.env.example`,
`.env.sample`, `.env.template`), private key and token files (`id_rsa*`,
`id_dsa*`, `id_ed25519*`, `id_ecdsa*`, and any name ending `.pem`,
`.key`, `.netrc`, or `.npmrc`, unless it ends `.pub`), and
`credentials*` files.

A refusal is a tool result, never a run abort: a sensitive-path or
`EscapesWorkspace` error round-trips to the model like every other
`is_error` result — it names the denied path or the allowed roots,
and nothing is ever opened on a refused path. A model that repeats a
refused call unchanged runs into the stationarity guard.

## Environment scrubbing

Tool children never see provider keys. The child environment is
filtered per call: `*_API_KEY` (and `API_KEY`) and the whole
`TEST_BYOK_*` family are removed; `GH_TOKEN` and everything unrelated
pass through in order.

## The loop

`Runner` drives one turn at a time to a terminal outcome. Knobs
(`RunConfig`): a system prompt, the turn cap (default 40), the total
token budget (default 4 000 000), the per-request output cap forwarded
to the provider (default 8192), the context window cap
(`max_context_tokens`, unset disables auto-compaction), the input-side
compaction limit (`max_input_tokens`, unset falls back to the context
cap), the thinking level string (`thinking_level`, unset keeps the
provider default), turn caps for the fold, an optional
idle timeout over a stalled stream, the step-attempt bound (default 3,
env `CRAFT_AGENT_STEP_RETRIES`), the result-window turn count (default
0 — trimming off; a nonzero window trades gateway cache affinity for
smaller prompts; env `CRAFT_AGENT_RESULT_WINDOW_TURNS`), the
per-result truncation cap (default 4096 bytes, env
`CRAFT_AGENT_RESULT_TRUNCATE_BYTES`, 0 disables; over-cap results ride
as head plus a spill pointer under RUNNER_TEMP), the workspace root,
and the session path. The tool layer reads `CRAFT_AGENT_READ_ROOTS`
from the environment once at startup: extra read-only roots for the
file-read tools. The four context knobs resolve CLI flag over agent-file
frontmatter (`maxContextTokens`, `maxInputTokens`, `maxOutputTokens`,
`thinking`) over environment (`CRAFT_AGENT_MAX_CONTEXT_TOKENS`,
`CRAFT_AGENT_MAX_INPUT_TOKENS`, `CRAFT_AGENT_MAX_OUTPUT_TOKENS`,
`CRAFT_AGENT_THINKING_LEVEL`) over the default; a garbage, zero, or
oversized value is a typed error naming the knob, and an output cap
above the provider's `max_tokens` field fails the same way.

Before each model call, when a context cap is set, the runner
estimates the used context — the provider's last-turn input report
when it is positive, else a chars/4 walk over the session — and past
the trigger (used at 85% of the effective input limit, or used plus a
50 000-token reserve at the context cap) it asks the model for one
summary of the history so far (output capped at min(max_output,
131072) tokens), replaces the history prefix with that summary
message, and keeps the last 4 messages verbatim. A `compacted` event
marks the swap. Compaction is fail-soft: a failed summary call counts
one attempt and the run continues, and past 3 attempts compaction
stops for the rest of the run. Each turn:

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
| `compacted` | `turn`, `attempt`, `kept_messages`, `summary_bytes` |
| `run_finished` | `exit_code`, `category`, `detail`, `usage`, `turns` |

`run_finished` is always the last line; its `exit_code` doubles as the
process exit code, and `category` carries the cancellation taxonomy:
`completed`, `error`, `max_turns`, `stationarity`, `budget`. A `turn_retried` line marks a
replayed model step: the assistant text the run already streamed for
that turn since its previous boundary is void, `attempt` is the 1-based
number of the failed attempt, and `reason` is a stable token —
`stream_closed`, `transport`, `transient`, `timeout`, `idle_timeout`,
`rate_limit`, or `empty_response`.

## Run summaries

When the environment sets `GITHUB_STEP_SUMMARY` — a GitHub Actions
runner does so for every step — each mission seam appends a markdown
record of the finished run to that file, and GitHub renders it in the
run page's summary area. An unset or empty variable changes nothing:
no file I/O, no extra output. There is no knob for this anywhere —
env presence is the whole switch, so `.env.example` gains nothing and
consumers pass no flags.

The record is assembled from the run's own terminal record and
session, never from the environment. The heading carries the
conclusion word and exit code (`## craft-agent — max-turns (exit 2)`);
a lead blockquote carries the outcome detail; a facts table carries
mission, model and provider, turns of the turn cap, the token buckets
of the budget, and the session path. The run mission adds a
Deliverables section — pull-request links
(`https://github.com/<owner>/<repo>/pull/<n>`) and branch-push
targets the transcript surfaced, deduped and capped at 8 — the final
assistant message capped at 2,000 characters, and a `<details>`
transcript tail capped at 4,000 characters. The workflow mission
adds a per-step table (step id, conclusion, turns, tokens) and draws
its deliverables from every settled step's session, with the failed
step's final message and tail at the end; the digest mission links
the digest markdown path instead. Every cap cuts on a character
boundary, and a summary that cannot be written is one stderr warning:
the run is already over and its exit code never moves.

One `::notice::<conclusion> — exit <code> — <first line>` annotation
rides along — stdout in plain mode, stderr in stream-json mode, where
stdout is the NDJSON contract and `run_finished` stays the last line;
both streams land in the Actions step log. The summary is public-safe
by construction: it carries the same content the NDJSON transcript
already carries, adds no environment values of its own, and GitHub
masks registered secrets that slipped into tool output exactly as it
masks them in the log.
