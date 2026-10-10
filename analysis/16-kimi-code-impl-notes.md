# kimi-code implementation notes (rs2-kimi-impl)

Deep-dive into `/home/ahmed/Projects/kimi-code` (Kimi Code CLI — TypeScript monorepo, MIT;
entry chain `apps/kimi-code/src/main.ts` → `cli/commands.ts` → `cli/v2/run-v2-print.ts`
(print mode) or `run-shell.ts` → TUI; engine is `packages/agent-core-v2`). All claims below
verified against source; goal is semantics to port into the storytold pure-Rust BYOK core.

Repo map relevant to us:
- `packages/agent-core-v2/src/agent/tools/**` — built-in tools (+ `*.md` description
  templates rendered into tool schemas).
- `packages/agent-core-v2/src/agent/permission{Rules,Policy,Mode,Gate}` — permission engine.
- `packages/agent-core-v2/src/features/externalHooks/**` — shell hook system.
- `packages/agent-core-v2/src/agent/{fullCompaction,contextMemory,loop,task}` — context,
  compaction, loop, background tasks.
- `packages/agent-core-v2/src/{llm-adapter,app/kosongConfig}` — providers + config.
- `packages/agent-core-v2/src/agent/profile/context.ts` — AGENTS.md discovery.
- `apps/kimi-code/src/cli/{options.ts,prompt-render.ts,v2/run-v2-print.ts}` — CLI flags,
  `stream-json` writer, print-mode runner.
- `packages/agent-core-v2/docs/{config-manifest.toml,wire-manifest.d.ts,state-manifest.d.ts}`
  — published contract manifests (the repo treats them as review tripwires).

## 1. Built-in tool set (v1 surface for our Rust core)

Tool names are the registered service names — `name: 'X'` in each `*Tool.ts` or
`contributeTool(..., { name: 'X' })` in each feature:

| Tool | Source | Responsibility |
| --- | --- | --- |
| `Read` | `agent/tools/os/read/readTool.ts:624` | Paged text read: `path`, `line_offset` (negative = tail), `column_offset`, `n_lines`, `max_chars` (default 100k, max 500k); UTF-16 auto-detect; refuses binary/NUL; line numbers in output; continuation args in status block |
| `Write` | `agent/tools/os/write/writeTool.ts:137` | Create/overwrite or `mode: append`; creates parent dirs; **requires prior `Read` of an existing file, rejects if file changed on disk since that read** (new-file writes exempt) |
| `Edit` | `agent/tools/edit/editTool.ts:116` | Exact `old_string`→`new_string` replacement; ambiguous match errors and suggests `replace_all`; same prior-Read + changed-on-disk guard |
| `Grep` | `agent/tools/os/grep/grepTool.ts:349` | ripgrep-backed content search; `output_mode: files_with_matches/content/count_matches`; `-A/-B/-C/-i/-n/multiline`; `offset`+`head_limit` paging (default 250); sensitive files always filtered; `include_ignored` opt-in |
| `Glob` | `agent/tools/os/glob/globTool.ts:330` | rg-powered file glob, mtime-desc sort, `offset`+`head_limit` (default 100, `0`=unlimited); respects `.gitignore/.ignore/.rgignore`; sensitive files filtered |
| `ReadMediaFile` | `agent/tools/read-media-file/readMediaFileTool.ts:184` | Send image/video as multimodal content; auto-compress to model limits; 100 MB cap |
| `Bash` | `agent/tools/os/bash/bashTool.ts:440` | Shell exec; `cwd`, `timeout` (fg default 60 s / max 300 s; bg default 600 s / max 86400 s; `disable_timeout` for bg), `run_in_background` + required `description`; stdin closed (EOF); SIGTERM → 5 s grace → SIGKILL; permission subject = raw `command` glob (`bashTool.ts:151`) |

Coordination / meta tools:

| Tool | Source | Responsibility |
| --- | --- | --- |
| `Agent` | `agent/tools/agent/agentTool.ts:664` | Spawn same-process subagent loop with its own context + wire file; `prompt`+`description` required, optional `subagent_type` (default `coder`), `resume`, `run_in_background`, `model` (subagent model pool); result visible only to caller (parent keeps handoff, not dumps); 2 h default timeout, no timeout in `-p` |
| `AgentSwarm` | `features/swarm/tools/agent-swarm/agentSwarmTool.ts:74` | Fan-out from one `prompt_template` with `{{item}}` per `items[]` entry (min 2, max 128); waits for all, returns aggregated report; `KIMI_CODE_AGENT_SWARM_MAX_CONCURRENCY` cap |
| `AskUserQuestion` | `agent/tools/ask-user-question/askUserQuestionTool.ts:256` | 1–4 structured questions, 2–4 options each, auto "Other"; `background: true` opens a question task |
| `NotifyUser` | `features/notify/tools/notify-user/notifyUserTool.ts:24` | Mid-turn user-facing progress note (experimental flag-gated) |
| `Skill` | `features/skill/tools/skillTool.ts:25` | Invoke an inline skill; `MAX_SKILL_QUERY_DEPTH` (3) nesting cap (`features/skill/tools/skill.ts:13-18`) |
| `TodoList` | `features/todo/todoFeature.ts:13` | Session-scoped `{title,status}` list; omit `todos` to query, `[]` to clear |
| `TaskList` / `TaskOutput` / `TaskStop` / `WaitFor` | `agent/tools/task/task-{list,output,stop,task-wait}/*Tool.ts` | Background-task registry: list (`active_only`, `limit` 1-100), non-blocking output snapshot (32 KB inline preview + `output_path`, full log on disk), stop (idempotent on terminal tasks), and bounded wait (`timeout` ≤ 90 s, optional `task_id`; timeout is not an error; steer-message ends wait) |
| `CronCreate` / `CronList` / `CronDelete` | `features/cron/cronFeature.ts:17-19` | Session-bound scheduled prompts; 5-field local-time cron; anti-herd jitter (`min(10% period, 15 min)` recurring; ≤90 s one-shot on :00/:30); coalesce missed fires into one `<cron-fire>` envelope; 50-task cap; `KIMI_DISABLE_CRON=1` |
| `EnterPlanMode` / `ExitPlanMode` | `features/plan/planFeature.ts:18-22` | Plan mode: `Write`/`Edit` restricted to the plan file, `TaskStop` blocked; exit presents plan + up to 3 alternative options |
| `CreateGoal` / `GetGoal` / `SetGoalBudget` / `UpdateGoal` | `features/goal/goalFeature.ts:27-39` | Headless goal contract with objective, budget, proof, stop rule; drives continuation turns and exit codes |
| `select_tools` (lowercase) | `agent/toolSelect/toolSelect.ts:6` | Runtime tool-surface narrowing (human/tool-select feature) |
| `WebSearch` / `FetchURL` | `agent/tools/web-search/webSearchTool.ts:115`, `agent/tools/fetch-url/fetchUrlTool.ts:79` | Host-injected capabilities — only registered when the host provides an implementation, else absent from the tool list |

Notes worth porting: MCP tool names follow `mcp__<server>__<tool>` (globs allowed in
activation lists; `agent/toolPolicy/evaluate.ts:79-104` flags wildcard-outside-mcp,
incomplete `mcp__server`, and unknown tool names). Tool result budget: 50k chars default
truncation with on-disk spill (`tool/toolContract.ts:7-9`).

## 2. Permission system

Three modes (`agent/permissionPolicy/types.ts:6`): `manual` ("Always Ask"), `yolo`
("Ask When Needed"), `auto` ("Never Ask"); default `manual` (`default_permission_mode` in
`~/.kimi-code/config.toml`, `docs/en/configuration/config-files.md:99`).

### Rule syntax — `[[permission.rules]]`

Parsed at `agent/permissionRules/configSection.ts:80-126` + `matchesRule.ts:32-56`:
- `decision`: `allow` | `deny` | `ask`; `scope`: `turn-override` | `session-runtime` |
  `project` | `user` (default `user`); optional `reason`.
- `pattern` = `ToolName` or `ToolName(arg-glob)`, e.g. `Read`, `Bash(rm -rf*)`.
  Shortcuts: `[[permission.deny]] / [[permission.allow]] / [[permission.ask]]` tables with
  `tool = "Bash"`, `match = "rm -rf*"` are normalized into the same rules
  (`configSection.ts:84-126`).
- Tool-name matching is glob (picomatch) with `*` wildcard; arg matching delegates to the
  tool's `matchesRule` (`matchesRule.ts:60-83`). Bash matches the raw command string;
  Read/Write/Edit match the **canonicalized path** with case-insensitive path-glob, `!`
  negation, `~` expansion, and `./` stripping (`tool/rule-match.ts:18-160`,
  `readTool.ts:229-235`, `writeTool.ts:61-67`, `editTool.ts:68-74`).
- Ordered: first matching rule for a given decision wins; deny policies run before ask/allow.
- `dangerousCommandGuard` (default `true`) + env `KIMI_CODE_DANGEROUS_COMMAND_GUARD`
  (`configSection.ts:45-69`).
- Docs: `docs/en/configuration/config-files.md:519-551`.

### Ordered policy chain (first non-undefined result wins)

`agent/permissionPolicy/permissionPolicyService.ts:39-56`, evaluated in this order:
1. `auto-mode-ask-user-question-deny` — in `auto` mode, `AskUserQuestion` is **denied**
   ("decide and continue") so headless runs never block on a question
   (`policies/auto-mode-ask-user-question-deny.ts`).
2. `user-configured-deny` — matching deny rules → hard deny
   (`policies/user-configured-rule.ts:20-40`; deny message includes the rule `reason`).
3. `dangerous-command-ask` — **only when interactive** (omitted entirely when
   `bootstrap.args.nonInteractive`): parse the bash command (own tree-sitter bash parser,
   500 ms/10k-node budget), unwrap sudo/doas/env/command/exec/nohup/nice/busybox, recurse
   nested `sh -c`/`eval` (depth 4), then ask on: shutdown-family commands, mkfs/wipefs,
   `init/telinit 0|6`, `systemctl poweroff/reboot/halt/kexec`, `dd of=/dev/*` (except
   null/zero/full/random/urandom/std{in,out,err}), `rm -rf` (safe only under /tmp or /temp,
   no `..`); **unparseable/unsafe-operand commands ask in manual mode**, are allowed in
   `yolo`, skipped in `auto` (`policies/dangerous-command-ask.ts:15-146`).
4. `auto-mode-approve` — in `auto` mode everything else is approved
   (`policies/auto-mode-approve.ts`).
5. `session-approval-history` — "approve for session" answers replay for the rest of the
   session as synthesized allow rules (`policies/session-approval-history.ts:16-39`;
   recorded durably via `permissionRulesOps.ts:59-70`).
6. `user-configured-ask` → 7. `user-configured-allow` — remaining rule decisions.
8. `sensitive-file-access-ask` — ask when any accessed path is sensitive
   (`policies/sensitive-file-access-ask.ts`).
9. `git-control-path-access-ask` — ask for any `.git` control-path access
   (`policies/git-control-path-access-ask.ts:28-49`).
10. `yolo-mode-approve` — everything approved in `yolo`.
11. `default-tool-approve` — a fixed allowlist that needs no approval even in `manual`
    mode: Read, Grep, Glob, ReadMediaFile, SetTodoList, TodoList, TaskList, TaskOutput,
    WaitFor, CronList, WebSearch, FetchURL, Agent, AgentSwarm, AskUserQuestion, NotifyUser,
    Skill, EnterPlanMode, ExitPlanMode, CreateGoal, GetGoal, SetGoalBudget, UpdateGoal,
    select_tools (`policies/default-tool-approve.ts:11-31`). Everything else
    (Write, Edit, Bash, TaskStop, CronCreate, MCP tools) asks.
12. `git-cwd-write-approve` — Write/Edit inside the workspace of a **non-git** dir are
    auto-approved (posix only) (`policies/git-cwd-write-approve.ts`).
13. `fallback-ask` — always `{kind: 'ask'}` (`policies/fallback-ask.ts`).

### Headless (`-p`/print) behavior

`apps/kimi-code/src/cli/v2/run-v2-print.ts:407-481`: print mode calls
`permissionMode.setMode('auto')` on the main agent (restored afterwards on resume paths) and
bootstraps with `nonInteractive: true` (line 171). Consequences:
- AskUserQuestion → deny (policy 1); everything else → auto-approve (policy 4) — so user
  `ask` rules never fire headless; **user `deny` rules still win** (policy 2 runs first).
- The dangerous-command guard is not even registered (policy 3 skipped headless).
- What stays effective headless regardless of mode:
  a. **Sensitive-file hard block** — not a permission ask at all; `resolvePathAccess`
     throws `PATH_SENSITIVE` for `.env*` (exemptions `.env.example/.env.sample/.env.template`),
     `id_rsa*`/`id_ed25519*`/`id_ecdsa*`/`credentials*` (+ `-`/`_`/dot-variant suffixes),
     `.aws/credentials`, `.gcp/credentials`; public `*.pub` keys exempted
     (`tool/path-access.ts:15-83`). Applies to Read, Write, Edit, Grep, Glob, and any tool
     that resolves paths.
  b. **Workspace guard** — relative paths that resolve outside the workspace are rejected
     (`PATH_OUTSIDE_WORKSPACE`); absolute outside-workspace paths are allowed
     (`path-access.ts:246-289`, policy `absolute-outside-allowed`).
  c. Tool activation policy (`tools`/`disallowedTools` layers: workspace → profile → global
     → session; `agent/toolPolicy/evaluate.ts:43-61`) and permission deny rules.
For our CI Action: replicate exactly this shape — auto-approve + deny AskUserQuestion +
hard sensitive-file/workspace errors + deny rules honored; that is the unattended safety
contract.

## 3. Hooks (shell hooks)

Config: `[[hooks]]` array in `~/.kimi-code/config.toml`; strict 4 fields — `event`,
`matcher?` (regex string), `command` (shell, run via `shell: true`), `timeout?` (1-600 s,
default 30) (`features/externalHooks/configSection.ts:10-21`;
`docs/en/customization/hooks.md:32-51`). Duplicate commands for the same event run once
(`internal/matchHooks.ts:47-53`); matcher is `new RegExp(pattern).test(matcherValue)`
(`matchHooks.ts:106-113`).

### Event list — 20 events, not 19 (`features/externalHooks/internal/types.ts:3-24`)

`PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PermissionRequest`, `PermissionResult`,
`UserPromptSubmit`, `UserPromptQueued`, `TurnStarted`, `Stop`, `StopFailure`, `Interrupt`,
`SessionStart`, `SessionEnd`, `SessionHeartbeat`, `SubagentStart`, `SubagentStop`,
`TaskStarted`, `PreCompact`, `PostCompact`, `Notification`.

### JSON contract

- stdin: one JSON object, all keys snake_case (camelCase converted by `camelToSnake`,
  `matchHooks.ts:124-134`). Base fields: `hook_event_name`, `session_id`, `session_title`,
  `client_type`, `cwd` + event-specific fields (`docs/en/customization/hooks.md:64-75`).
  e.g. PreToolUse: `{tool_name, tool_input, tool_call_id, ...}`; PostToolUse:
  `{tool_name, tool_input, tool_call_id, tool_output (≤2000 chars)}`; failure variant adds
  `error` (`agent/agentExternalHooksService.ts:301-333`).
- Exit codes: **2 = block** (stderr text becomes the reason); other non-zero / timeout /
  spawn-error = allow (**fail-open**) (`internal/runHook.ts:140-173, 30-31`).
- Exit 0 + stdout JSON `{"message": "...", "hookSpecificOutput": {"message": "...",
  "permissionDecision": "deny", "permissionDecisionReason": "..."}}` — deny blocks
  (`runHook.ts:43-56, 175-206`). Plain stdout on exit 0 is an advisory message.
- Blockable events only: `PreToolUse`, `Stop`, `UserPromptSubmit`
  (`docs/en/customization/hooks.md:101-105`). Everything else is fire-and-forget.
- Kill: SIGTERM then SIGKILL after 100 ms grace; process-group detached on non-Windows
  (`runHook.ts:16-28, 229-235`).

### Semantics that matter for CI

- `PreToolUse` runs **before permission checks**; a block vetoes the tool call outright
  (`agentExternalHooksService.ts:160-168, 301-316`).
- `UserPromptSubmit` on allow can **inject advisory context**: each hook's message (or raw
  stdout) becomes a `<hook_result hook_event="...">` part appended to the prompt
  (`internal/userPrompt.ts:22-39`; `agentExternalHooksService.ts:370-383`). On block, an
  assistant message carrying the reason is recorded and the turn never launches.
- `Stop` block reason is re-injected as a user message (`origin: system_trigger,
  name: stop_hook`) and the loop continues — a "keep working" latch, once per turn
  (`agentExternalHooksService.ts:237-259, 409-420`).
- `PostToolUse` = observation + advisory additionalContext (via the same message channel);
  `PostToolUseFailure` fires for errors and permission-blocked calls.
- `SessionHeartbeat` every 60 s only when configured (`sessionExternalHooksService.ts:25, 99-107`).

## 4. Session / context management

### Transcript persistence — append-only wire journal (JSONL)

One file per agent: `<home>/sessions/wd_<workspacename>_<hash>/session_<uuid>/agents/
<agentId>/wire.jsonl` (layout `workspace/sessionLifecycle/internal/addressing.ts:16-25`;
backend appends lines, atomic `.tmp`+rename for full writes,
`human/store/backend/node.ts:45-80`; live sample verified under `~/.kimi-code/sessions/`).
First line is a metadata record `{"type":"metadata","protocol_version":"1.4","created_at":
...}`, then `config.update` (system prompt snapshot) and event records. Record types seen
in the recovery footer (`agent/fullCompaction/context-recovery-footer.md`):
`context.append_message` (user prompts), `context.append_loop_event` (event.type:
`step.begin | content.part | tool.call | tool.result | step.end`), `context.apply_compaction`,
`context.undo`, `context.clear`, plus bookkeeping (`llm.request`, `usage.record`,
`token_counting.measured`, `metadata`, `profile.bind`). Design point: **the event log is
never rewritten** — compaction/undo append records; older lines stay on disk so the agent
can grep them (the Context Recovery footer teaches the model exactly that).

### Compaction (full compaction)

- Trigger: `usedSize >= maxSize * triggerRatio` (default **0.85**; per-model override via
  `compactionTriggerRatio`) **or** `usedSize + reservedContextSize >= maxSize`
  (reserved default 50k tokens) — `agent/fullCompaction/strategy.ts:18-28, 116-135, 91-101`.
  `blockRatio` 0.85 blocks/forces compaction before overflow; overflow retries capped at 3
  with ≥5% reduction required (`strategy.ts:23-27, 178-196`).
- What is kept: a **recent-message window** chosen by walking back over splittable
  boundaries — at most `maxRecentMessages` 4 messages, capped at 20% of window size, all
  recent user messages; splits never cut an assistant↔tool exchange
  (`strategy.ts:137-176, 242-265`).
- What replaces the compacted prefix: a model-written **handoff summary** generated by the
  `compaction-instruction.md` prompt (intent of latest request, in-force constraints,
  done-vs-unknown at high fidelity, open questions, forward plan with exact next command;
  "be honest about unverified claims"). The summary is prefixed by a fixed caution header
  (`agent/contextMemory/compaction-summary-prefix.md`: treat as notes not proof) and
  followed by the Context Recovery footer pointing at the wire file; TODO list is
  re-attached live, never transcribed.
- Hooks: `PreCompact`/`PostCompact` fire around it (`agentExternalHooksService.ts:262-272`).
- Config: `[loop_control] reserved_context_size`, `compaction_max_attempts` (default 5),
  `max_steps_per_turn` (0=unlimited), `max_attempts_per_step` (default 10);
  retries only for transient errors (conn/timeout/429-rate-limit/5xx — not quota 429)
  (`docs/en/configuration/config-files.md:321-332`).

### Background tasks + print-mode policy

- Background tasks (Bash `run_in_background`, background `Agent`, question tasks) notify
  the agent automatically on completion (synthetic user message).
- After the main turn settles, `-p` applies the background policy
  (`apps/kimi-code/src/cli/v2/run-v2-print.ts:876-971`): mode `exit` (return now) |
  `drain` (suppress notifications + wait for tasks within ceiling) | `steer` (stay alive
  while tasks pending so completions steer new main turns); default `steer`
  (`keepAliveOnExit=true` maps to `drain`; `agent/task/configSection.ts:40-44`).
  Ceiling default = `PRINT_WAIT_CEILING_S_DEFAULT` (max JS timer delay ≈ 24.8 d,
  `agent/task/printDefaults.ts:9`), `printMaxTurns` 100 000; env overrides
  `KIMI_CODE_BACKGROUND_PRINT_{BACKGROUND_MODE,WAIT_CEILING_S,MAX_TURNS}` etc.
  (`agent/task/configSection.ts:46-79`). Steered turns that fail → the run fails.
- Session resume: `--session <id>` (validates the session's cwd matches the current dir),
  `--continue` picks the most recent session with matching cwd; agent profile + bound model
  restored by the engine (`run-v2-print.ts:399-467`). Session index at
  `<home>/session_index.jsonl` (`workspace/workspaceAlias.ts:10`).

## 5. `--output-format stream-json` (print mode JSONL contract)

Writer: `apps/kimi-code/src/cli/prompt-render.ts:149-267` (`PromptJsonWriter`), fed by the
native event bus (`run-v2-print.ts:681-727`). One JSON object per stdout line:

| Line | Shape | When |
| --- | --- | --- |
| version meta | `{"role":"meta","type":"system.version","version":"..."}` | first line of every run (`prompt-render.ts:358-388`) |
| assistant text | `{"role":"assistant","content":"..."}` | flushed at step boundaries / before tool results / finish (`writeAssistantDelta` buffers, `flushAssistant` at `turn.step.started`, `turn.step.interrupted`, `tool.result`, end) |
| tool calls | same assistant line with `tool_calls:[{"type":"function","id":"...","function":{"name":"...","arguments":"<json-str>"}}]` | accumulated from `tool.call.started` + `tool.call.delta` (arguments streamed as string parts) |
| tool result | `{"role":"tool","tool_call_id":"...","content":"<stringified output>"}` | on `tool.result`; string output verbatim, structured output JSON-stringified |
| retry meta | `{"role":"meta","type":"turn.step.retrying","failed_attempt":n,"next_attempt":n,"max_attempts":n,"delay_ms":n,"error_name":"...","error_message":"...","status_code":n}` | on provider retry; the failed attempt's partial assistant text is discarded first |
| resume hint | `{"role":"meta","type":"session.resume_hint","session_id":"...","command":"kimi -r <id>","content":"..."}` | last line (`prompt-render.ts:390-410`) |

Details worth keeping: thinking deltas are NOT written to JSONL (stderr only in text mode);
hook results are written as assistant messages `<event> hook [blocked]\n\n<content>`
(`prompt-render.ts:159-165, 333-344`); tool progress and warnings go to stderr; there is
**no cost/usage line** in the current contract — usage exists internally
(`agent/usage/usageOps.ts:21` `usage.record`, per-model TokenUsage incl. cache tokens) and is
surfaced by the TUI/kap-server, not by `-p`. The writer is engine-agnostic (v1 and v2 event
streams feed the same interface), and the repo pins it as an external contract
(`apps/kimi-code/src/cli/` is on the tripwire list, root `AGENTS.md:112`).
Output format precedence: `--output-format` flag → `KIMI_MODEL_OUTPUT_FORMAT` env (prompt
mode only) → `text`; flag rejected outside `-p` (`cli/options.ts:21-35, 76-78`).

## 6. Providers & config

### config.toml shape

File: `~/.kimi-code/config.toml` (relocatable via `KIMI_CODE_HOME`;
`app/bootstrap/bootstrap.ts:116`). No project-level config.toml — only `mcp.json` is
project-local (`docs/en/configuration/overrides.md:31-33`). Schemas:
`app/kosongConfig/configSection.ts:42-57` (providers), `:151-195` (models).

`[providers.<name>]`: `type`, `api_key`, `api_key_env`, `base_url`, `oauth{storage,key}`,
`env` (sub-table), `custom_headers`, `model_source`, `default_model`.
`[models.<alias>]`: `provider`, `model`, `max_context_size` (required), `max_input_size`,
`max_output_size`, `capabilities`, `support_efforts`/`default_effort`/`off_effort`,
`base_url` (per-model override), `display_name`, `reasoning_key`, `adaptive_thinking`,
`beta_api`, plus a partial `[models.<alias>.overrides]` patch table.

### Provider kinds

Protocols (wire formats) are exactly four: `anthropic`, `openai` (Chat Completions),
`openai_responses`, `google-genai` (`llm-adapter/protocol/protocol.ts:11`).
Registered provider definitions (`llm-adapter/provider/provider-definition.ts:197-253`):
- `anthropic` — Anthropic Messages; env `ANTHROPIC_API_KEY` / `ANTHROPIC_BASE_URL`
  (base URL overridable — in our port the endpoint stays fixed per requirements, but the
  base_url mechanism is the same).
- `openai` — Chat Completions; env `OPENAI_API_KEY` / `OPENAI_BASE_URL` (overridable
  base for OpenAI-compatible gateways); auto `reasoning_content`/`reasoning_effort`
  handling; per-model `reasoning_key` for nonstandard gateways.
- `openai_responses` — Responses API, same env pair.
- `google-genai` — Gemini; endpoint list `[vertexEndpoint, geminiEndpoint]`:
  `VERTEXAI_API_KEY`/`GOOGLE_VERTEX_BASE_URL` tried first, then
  `GOOGLE_API_KEY`/`GOOGLE_GEMINI_BASE_URL`; SDK default
  `https://generativelanguage.googleapis.com` (host root only — SDK appends
  `/v1beta/models/<model>:generateContent`).
- `vertexai` — a provider **type string** (docs list it as a type), not a separate
  protocol: routes to google-genai with `providerOptions.vertexai=true` + project/location
  from `[providers.<n>.env]` (`model/catalog-service.ts:568-576`;
  `protocol/protocolAdapterRegistry.ts:124-135`).
- `kimi` — the built-in managed provider, registered under all three of
  openai/anthropic/openai_responses protocols.

### Credential resolution (BYOK semantics to port)

`llm-adapter/model/model-auth.ts:21-78` + `packages/oauth/src/provider-credential.ts:28-60`:
model.apiKey → model.oauth → provider `api_key` (inline) → provider `api_key_env`
(**indirection: name of a shell env var, re-read every request, unset/empty fails fast
naming the provider+var**) → provider `oauth` → protocol endpoint env fallback
(e.g. `ANTHROPIC_API_KEY`). `apiKey`/`apiKeyEnv`/`oauth` are mutually exclusive —
combining two is a config conflict error. Credentials never implicitly read shell env
except via `api_key_env`; base_url resolves `providers.<n>.base_url` then the matching
`*_BASE_URL` key in the `env` sub-table (`docs/en/configuration/overrides.md:36-52`).
Priority for ordinary params: CLI flags > config.toml; ordinary params never fall back to
shell env except explicitly modeled env bindings (`overrides.md:21-33`).

### KIMI_* env overlay (`app/kosongConfig/envOverlay.ts`)

A registered `ConfigEffectiveOverlay` that **synthesizes config from env at load time**,
then strips it when persisting (`envOverlay.ts:87-203, 190-202`):
- `KIMI_MODEL_NAME` → creates alias `__kimi_env_model__` on pseudo-provider `__kimi_env__`
  and sets `defaultModel` to it; provider type from `KIMI_MODEL_PROVIDER_TYPE` (default
  `kimi`), base URL from `KIMI_MODEL_BASE_URL` (falls back to the provider's protocol
  default endpoint), key from `KIMI_MODEL_API_KEY`.
- Model knobs: `KIMI_MODEL_MAX_CONTEXT_SIZE` (default 262 144),
  `KIMI_MODEL_MAX_OUTPUT_SIZE`, `KIMI_MODEL_CAPABILITIES` (csv; default
  `image_in,thinking`), `KIMI_MODEL_DISPLAY_NAME`, `KIMI_MODEL_REASONING_KEY`,
  `KIMI_MODEL_ADAPTIVE_THINKING`.
- Request knobs (work with or without `KIMI_MODEL_NAME`): `KIMI_MODEL_TEMPERATURE`,
  `KIMI_MODEL_TOP_P`, `KIMI_MODEL_THINKING_KEEP`, `KIMI_MODEL_THINKING_EFFORT`
  (configSection.ts:329-331), `KIMI_MODEL_MAX_COMPLETION_TOKENS` /
  `KIMI_MODEL_MAX_TOKENS` → `modelOverrides`.
This is the exact BYOK pattern our Action needs: zero-config-from-env with the synthesized
state never written back to the user's config.

## 7. AGENTS.md / skills discovery

### AGENTS.md load order (`agent/profile/context.ts:137-196`)

1. `<kimiHome>/AGENTS.md` (i.e. `~/.kimi-code/AGENTS.md`) — global brand instructions.
2. First existing of `~/.agents/AGENTS.md`, `~/.agents/agents.md` — user generic.
3. Per work dir: find the **git work tree root** (`findGitWorkTree`; falls back to the dir
   itself), then walk **root → leaf** from worktree root down to the cwd; in each dir
   collect `.kimi-code/AGENTS.md` and then `AGENTS.md` or `agents.md` (**first plain-name
   hit wins per dir**, `:108-123`).
All files are concatenated with `<!-- From: <path> -->` annotations
(`renderAgentFiles`, `:316-332`); empty/unreadable files are skipped with warnings; a
soft warning appears when the total exceeds 32 KB (`AGENTS_MD_RECOMMENDED_MAX_BYTES`,
`:8, 186-193`). The same roots are watched for live reload (200 ms debounce;
`workspace/workspaceInstructions/workspaceInstructionsService.ts:21-120`). This search
order matches how the storytold org repos (root + per-dir AGENTS.md) are consumed.

### Skills discovery (`features/skill/catalog/skillRoots.ts:8-39`)

- User: `<kimiHome>/skills` (brand) **and** `~/.agents/skills` (generic).
- Project: `.kimi-code/skills` and `.agents/skills` at the **git worktree root** (both
  merged when `mergeAllAvailableSkills` is true, else first existing).
- `--skills-dir` (repeatable) **replaces** discovery; relative config paths resolve
  against the project root; plugin sources and built-in product skills merge in too.
- Inline skills are callable via the `Skill` tool (max nesting depth 3).

## 8. Other port-worthy details

- Tool result spill: results over 50k chars are truncated for context with an
  `output_path` to the full saved log (`tool/toolContract.ts:7-9`,
  `agent/toolResultTruncation/toolResultTruncationService.ts:6-21` — 4k head + 1k tail
  preview); `TaskOutput` inline preview is 32 KB with full log on disk.
- Workspace trust: untrusted folders drop project-level MCP servers with an explicit
  stderr warning in print mode (`run-v2-print.ts:280-288, 496-530`);
  `KIMI_CODE_TRUST_WORKSPACE=1` pre-trusts. Relevant to CI: assume trusted root.
- Flag conflicts rejected up-front (`cli/options.ts:64-120`): `--prompt` with
  `--yolo/--auto/--plan`, `--continue`+`--session`, `--yolo`+`--auto`,
  `--agent`+`--agent-file`, agent flags with resume flags, `--output-format` outside `-p`.
- Exit codes: goal runs set `process.exitCode` from goal status
  (`run-v2-print.ts:675-677`); provider safety filter surfaces as "Provider safety policy
  blocked the response." (`run-v2-print.ts:973-982`).

## 9. PORT THIS — checklist for the Rust core

### Tools (v1 surface, in priority order)
- [ ] `Read` (paged, line-numbered, max_chars budget, continuation args), `Write`
      (overwrite/append, prior-Read + mtime guard), `Edit` (unique-match, `replace_all`),
      `Grep` + `Glob` (ignore-aware, sensitive-file filtered, offset/head_limit paging).
- [ ] `Bash`: fg 60 s default/300 s cap, bg 600 s default/86400 s cap, `disable_timeout`,
      closed stdin, SIGTERM→grace→SIGKILL, `run_in_background` + `description`.
- [ ] `Agent` subagent tool (own context + transcript file, resume, background mode,
      caller-only result). Defer `AgentSwarm`, `select_tools`, goal/plan/cron/notify for v2;
      port `TodoList`, `TaskList`/`TaskOutput`/`TaskStop`/`WaitFor` (they are cheap and the
      PR-bot loop needs them), `AskUserQuestion` present-but-denied in headless mode.
- [ ] Tool result truncation 50k chars + on-disk spill + `output_path` in result.

### Permissions (unattended-CI-safe, port the chain order verbatim)
- [ ] Modes `manual|yolo|auto`; headless flag forces `auto` + `nonInteractive`.
- [ ] Ordered chain: ask-user-question-deny → user-deny → [dangerous-command-ask only if
      interactive] → auto-approve → session-approval-history → user-ask → user-allow →
      sensitive-file-ask → git-control-path-ask → yolo-approve → default-tool-approve →
      git-cwd-write-approve → fallback-ask. In `auto`: deny rules still honored, everything
      else auto-approved, AskUserQuestion denied.
- [ ] `[[permission.rules]]` TOML: decision/scope/pattern/reason; `Tool`/`Tool(glob)`
      patterns; per-tool match subject (Bash=command glob; file tools=canonical path glob,
      case-insensitive, `!` negation); `allow/deny/ask` shortcut tables.
- [ ] Dangerous-command guard (interactive only) via a real bash parser, not regexes —
      port the command list + unwrapping + unanalyzable→ask semantics.
- [ ] Hard errors independent of permissions: sensitive-file denylist (.env* with
      example/sample/template exemptions, id_*, credentials*, .aws|.gcp/credentials),
      relative-path-outside-workspace rejection; absolute paths allowed.

### Hooks
- [ ] `[[hooks]]` (event, matcher-regex, command, timeout 1-600 s default 30); JSON on
      stdin (snake_case, base fields hook_event_name/session_id/cwd/session_title);
      exit 2 = block (stderr=reason); exit 0 + stdout JSON `hookSpecificOutput.
      permissionDecision:"deny"` = block; fail-open on error/timeout.
- [ ] Ship at least: `PreToolUse` (pre-permission veto), `PostToolUse` (advisory
      additionalContext ≤2k chars), `UserPromptSubmit` (advisory context injection +
      block), `Stop` (continue-latch), `SessionStart`/`SessionEnd`, `PreCompact`/
      `PostCompact`, `Notification`. Keep the other event names reserved.

### Output schema (stream-json, spirit-compatible)
- [ ] JSONL lines: `{role:assistant,content,tool_calls[]}` / `{role:tool,tool_call_id,
      content}` / `{role:meta,type:system.version|turn.step.retrying|session.resume_hint}`.
      Consider adding a `usage` meta line (kimi omits it) — additive, keeps consumers
      parsing `role` happy. Exit code semantics for goal/failed turns documented up front.

### Session/context
- [ ] Append-only JSONL wire journal per agent (metadata header line, then events;
      compaction/undo as appended records, never rewrite); session index file; resume by
      id with cwd match + `--continue` by cwd.
- [ ] Compaction: trigger at 0.85 ratio or `used+reserved≥max` (reserved 50k); keep ≤4
      recent messages ≤20% of window, never split tool exchanges; model-written handoff
      summary with the honesty/caution prefix + on-disk recovery pointer; `PreCompact`/
      `PostCompact` hooks.
- [ ] Print background policy: `exit|drain|steer` (default steer) with wall-clock ceiling
      + max-steered-turns caps; steered-turn failure fails the run.

### Config/providers
- [ ] `config.toml` at a portable home dir; `[providers.<n>]` type/api_key/api_key_env/
      base_url/env/custom_headers; `[models.<alias>]` with `max_context_size`;
      mutual exclusion of api_key/api_key_env/oauth with fail-fast errors naming both.
- [ ] Provider kinds: `anthropic` (Messages, fixed endpoint), `openai` (Chat Completions,
      base_url overridable), `google-genai` (Gemini, fixed host-root endpoint), plus
      `openai_responses`/`vertexai` cheap to add later via the same protocol enum.
- [ ] `api_key_env` indirection re-read per request (this is the .env/secret channel for
      our Action + integration tests); no implicit shell-env fallback anywhere else.
- [ ] Env overlay synthesis (KIMI_MODEL_* style: name, provider type, base URL, key, max
      context, temperature/top_p/max tokens) that never persists synthesized state.

### AGENTS.md/skills
- [ ] Discovery order: brand-home AGENTS.md → `~/.agents/AGENTS.md|agents.md` →
      git-root-to-cwd walk collecting `.kimi-code/AGENTS.md` then `AGENTS.md|agents.md`;
      concatenate with `<!-- From: path -->` markers; warn over 32 KB.
- [ ] Skill roots: user `<home>/skills` + `~/.agents/skills`; project `.agents/skills`
      (and `.kimi-code/skills`) at the git root; `--skills-dir` replaces discovery.
