# Candidate engine study: kimi-code (Kimi Code CLI)

Explored: `/home/ahmed/Projects/kimi-code` (github.com/MoonshotAI/kimi-code, MIT, tip commit c9f01f073, 2026-10-10). All claims cite files in that checkout. Slug: ex5-kimi-code.

## 1. What it is

- Terminal AI coding agent from Moonshot AI: reads/edits code, runs shell commands, searches files, fetches web pages, chooses next steps from feedback. Works with Kimi models and other providers. (`README.md:11-13`)
- **License: MIT** (`LICENSE`, "Copyright (c) 2026 Moonshot AI") — code and patterns are freely reusable, even relicensable, for the storytold project. Monorepo root is `@moonshot-ai/monorepo` v0.1.1 (`package.json:name/version/license`).
- **Runtime: Node.js >= 24.15.0, package manager pnpm 10.33.0** (`package.json:engines`, `packageManager`; `.nvmrc` = 24.15.0). Not bun. TypeScript throughout, pnpm workspaces (`pnpm-workspace.yaml`).
- Published npm CLI: `@moonshot-ai/kimi-code` v2.1.1, `bin: kimi` (`apps/kimi-code/package.json:2-8`). Also shipped as a **native single binary** via Node SEA (build scripts in `apps/kimi-code/scripts/native/`), so end machines need no Node at all (`README.md:31`, native scripts).
- Install: `curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash` or `npm install -g @moonshot-ai/kimi-code` (`README.md:17-24`, `docs/en/guides/getting-started.md:52`).

## 2. Monorepo layout (roles from each package.json name/description)

**apps/**
- `kimi-code` — `@moonshot-ai/kimi-code`, "The Starting Point for Next-Gen Agents": the CLI/TUI app. Must consume the engine only via `@moonshot-ai/kimi-code-sdk` (root `AGENTS.md:17`, `apps/kimi-code/AGENTS.md` "Module Responsibilities").
- `kimi-inspect` — web inspector for the kap-server debug RPC surface (`AGENTS.md:20`).
- `vis` (+ `apps/vis/server`, `apps/vis/web`) — visual debugging tools for sessions and replays (`AGENTS.md:19`).
- `vscode` — official VS Code extension (`apps/vscode/package.json`).

**packages/** (all `private: true` workspace packages, except the CLI; only the CLI is npm-published — verified via `grep '"private": true'` across all package.json + `.changeset/config.json`)
- `agent-core-v2` — "The unified agent engine for Kimi (v2 — DI Scope architecture)". The agent loop, tools, permissions, features. The heart of the system.
- `kosong` — "The LLM abstraction layer" (provider/model abstraction; the `KIMI_MODEL_*` env overlay lives in agent-core-v2/src/app/kosongConfig/envOverlay.ts).
- `kaos` — "Execution environment abstraction for AI agent applications" (file/process abstractions).
- `kap-server` — "Kimi Code server backed by the DI × Scope agent engine": REST + WebSocket session server (`/api/v1`, `/api/v1/ws`), debug RPC with loopback bind + bearer auth (`AGENTS.md:28`).
- `node-sdk` — "TypeScript SDK for the Kimi Code Agent": `KimiHarness` + `Session` facade over the engine.
- `klient` — "Kimi client SDK — contract-driven facade over agent-core-v2" (ipc/memory transports, hosts e2e suites).
- `oauth` — "Kimi OAuth toolkit for managed Kimi Code authentication" (device-code flow, device id, headers).
- `acp-server` — "Agent Client Protocol (ACP) host backed directly by the DI × Scope agent engine" — Zed/JetBrains IDE integration (`docs/en/reference/kimi-acp.md`).
- `pi-tui` — TUI library with differential rendering.
- `transcript` — isomorphic transcript rendering data layer (L1–L4).
- `telemetry`, `minidb` (embedded JSON store for kap-server search), `remote-control` (tunnel client), `tree-sitter-bash` (pure-TS bash parser used by the dangerous-command guard), `migration-legacy`.

**plugins/** — `marketplace.json` (official + curated plugin index) and `official/` (kimi-datasource, kimi-webbridge; a plugin is a directory with `kimi.plugin.json` manifest that bundles skills, agents, prompt instructions, and MCP servers).

**DI × Scope architecture** (important for reuse): agent-core-v2 organizes everything into four `LifecycleScope` tiers — App / Workspace / Session / Agent (`packages/agent-core-v2/src/app/scopes.ts`) with L3 `Service`/`Fiber` units and `Feature` seams in `src/features/` (root `AGENTS.md:21`). Callers compose `ISessionIndex` → `IWorkspaceLifecycleService.handlerFor` → handler. No App-level session facade.

## 3. Agent loop architecture

- Entry chain: `apps/kimi-code/src/main.ts` → `src/cli/commands.ts` (commander program) → `src/cli/run-prompt.ts` (print mode) or `src/cli/run-shell.ts` (TUI, via SDK `KimiHarness`) (`apps/kimi-code/AGENTS.md` entry chain).
- **The turn loop is a state machine**, not a while-loop: `createAgentMachine` + `createTurnMachine` from `#/human/agent/{machine,turn}` driven by `IAgentLoopService` (`packages/agent-core-v2/src/agent/loop/machine/engine.ts:13-14`). A prompt is enqueued via `loop.submit({message...})`, the caller awaits `handle.launched` then `turn.result` (`apps/kimi-code/src/cli/v2/run-v2-print.ts:557-568`).
- Loop outcomes: `LoopRunResult` = `completed | failed | cancelled` with step counts and `stopReason` (`src/agent/loop/loop.ts:107-120`). `max_steps_per_turn` guard raises `LOOP_MAX_STEPS_EXCEEDED` (`loop.ts:55-63`); configurable via `[loop_control]` in config.toml or `KIMI_LOOP_MAX_STEPS_PER_TURN` env (`docs/en/configuration/config-files.md:321-330`).
- Per-step machinery: LLM request via `IAgentLLMRequesterService` (retry/backoff, credential recovery — `src/agent/llmRequester/llmRequesterService.ts`), tool calls executed via `IAgentToolExecutorService`, streaming deltas (assistant/thinking/toolCall) surfaced as `MachineEngineDelta` events (`machine/engine.ts:50-80`).
- **Tools**: built-in tool implementations live in `src/agent/tools/` (Read/Write/Edit/Glob/Grep under `tools/os/`, Bash under `tools/os/bash/`, plus FetchURL, WebSearch, ReadMediaFile, plan-mode tools, select-tools; `docs/en/reference/tools.md` is the full list). Tool contract: `ToolDefinition`/`ExecutableTool` with `resolveExecution()`, zod-validated args, `source: builtin | user | mcp` (`src/tool/toolContract.ts:95-116`). Docs list: Read, Write, Edit, Grep, Glob, ReadMediaFile, Bash, WebSearch/FetchURL (host-injected), EnterPlanMode/ExitPlanMode, TodoList, Agent, AgentSwarm, AskUserQuestion, NotifyUser, Skill, TaskList/TaskOutput/TaskStop/WaitFor, CronCreate/CronList/CronDelete, Tower* (experimental).
- **Subagents**: `Agent` tool (foreground/background, `run_in_background`), `AgentSwarm` fan-out (up to 128 subagents, one template + items; `KIMI_CODE_AGENT_SWARM_MAX_CONCURRENCY` cap; `docs/en/reference/tools.md:99-103`). Built-in profiles: `agent` (main, `subagents: ['coder','explore','plan']`), `coder` (the only subagent with file-editing tools), `explore` (read-only prompt-enforced), `plan` (no shell, no edit) (`src/session/agentLifecycle/profile/profiles.ts:90-121`, `src/features/plan/profile/plan.ts:8-42`). Subagent handoff discipline is encoded in the shared role prefix (final message is the entire handoff; `profiles.ts:84-86`).
- Custom agent profiles: markdown files with frontmatter loaded via `--agent <name>` / `--agent-file <path>` (`cli/commands.ts:86-109`; engine-side `parseAgentFileText`).
- **Context management**: automatic compaction when remaining context falls below `reserved_context_size`, with `compaction_max_attempts` (`docs/en/configuration/config-files.md:321-330`); compaction machinery in `src/agent/contextMemory/` and `src/agent/fullCompaction/`; tool-result truncation budget in `src/agent/toolResultTruncation/`; token counting strategy config in `src/features/tokenCounting/`.
- Instructions: project `AGENTS.md`/`agents.md`/`.kimi-code/AGENTS.md` discovered from workdir up to the git root, plus user-level `~/.agents/AGENTS.md` and `~/.kimi-code/AGENTS.md` (`src/agent/profile/context.ts:205-227`) — per-repo guidance for 22 Rust repos would work out of the box.

- Runtime features via `src/features/`: plan, goal, todo, cron, skill, swarm, fileHistory, sessionInit, externalHooks, notify, reminder, tower, usage, tokenCounting, btw, dateChange, debugEvents — each a `Feature` contribution assembled by `featureAssemblyService`.

## 4. Headless / non-interactive mode

- **Flag: `-p, --prompt <prompt>`** — "Run one prompt non-interactively and print the response" (`apps/kimi-code/src/cli/commands.ts:64-69`). Enters `uiMode: 'print'` (`src/cli/options.ts:120`).
- **`--output-format text|stream-json`** — prompt-mode output format; `stream-json` emits one JSON object per stdout line (assistant messages with tool_calls, then Tool messages, then assistant text); thinking is never in JSONL; tool progress and resume notices go to stderr (`docs/en/reference/kimi-command.md:127-130`). Env default: `KIMI_MODEL_OUTPUT_FORMAT` (prompt mode only; `src/cli/options.ts:4-35`).
- Other relevant flags: `-m/--model`, `-S/--session [id]` resume, `-c/--continue`, `--skills-dir <dir>` (repeatable), `--agent/--agent-file`, `--add-dir <dir>` (repeatable extra workspace dirs), `--plan`. (`src/cli/commands.ts:42-121`)
- **Print mode always runs at `auto` permission (never asks)**: the runner forces `permissionMode.setMode('auto')` on the session's agent (`src/cli/v2/run-v2-print.ts:410-412,481`), which is why `--prompt` conflicts with `--yolo`, `--auto`, and `--plan` (rejected in `src/cli/options.ts:79-86`). Docs confirm: "In `-p` mode, no human approval is requested — regular tool calls are handled under the `auto` permission policy, while static deny rules remain in effect." (`docs/en/reference/kimi-command.md:118-120`)
- **Print-mode defaults auto-unbound for CI**: `applyPrintModeConfigDefaults` sets bash task timeout 0 (no timeout), subagent timeout 0, swarm timeout 0, `maxStepsPerTurn` 0 = unlimited — only for keys the user left unset (`src/agent/task/printDefaults.ts`).
- Background tasks after the main turn: policy `exit | drain | steer` (`src/agent/task/configSection.ts:17`), default `steer`; `printWaitCeilingS` and `printMaxTurns` bounds (envs `KIMI_CODE_BACKGROUND_PRINT_BACKGROUND_MODE`, `..._PRINT_WAIT_CEILING_S`, `..._PRINT_MAX_TURNS`, `src/agent/task/configSection.ts:44-49`).
- **Exit codes**: 0 on success; 1 on any thrown failure (set synchronously, `apps/kimi-code/src/main.ts:249-262`); option-conflict errors print `error: ...` and exit 1 (`main.ts:69-73`). Goal mode (`kimi -p "/goal ..."`): 0 complete / 3 blocked / 6 paused (`src/cli/goal-prompt.ts:26-30`). Signals: SIGINT 130, SIGHUP 129, SIGTERM 143 (`src/cli/run-prompt.ts:90-94`). A force-exit backstop drains stdio then kills a wedged event loop (`src/cli/headless-exit.ts`).
- Resilience for long runs: `KIMI_CODE_INFINITE_RETRY=1` retries failed LLM requests indefinitely, exponential backoff capped at 32 s, honoring Retry-After (`packages/agent-core-v2/src/agent/llmRequester/llmRequesterService.ts:529`, `docs/en/configuration/env-vars.md:171`).
- Workspace trust: headless runs cannot show the trust prompt; project-level `.kimi-code/mcp.json` servers stay disabled unless the workspace is already trusted or `KIMI_CODE_TRUST_WORKSPACE=1` is set (`docs/en/customization/mcp.md`, `run-v2-print.ts:524-529`).

**Exact CI invocation** (BYOK entirely via env; no config file needed):

```sh
kimi -p "Review this PR and comment on obvious bugs." \
  --output-format stream-json            # or text
# env: KIMI_MODEL_NAME=... KIMI_MODEL_API_KEY=${{ secrets.KEY }} KIMI_MODEL_BASE_URL=...
#       KIMI_CODE_HOME=$RUNNER_TEMP/kimi-home  (isolated state)
#       KIMI_CODE_TRUST_WORKSPACE=1  KIMI_DISABLE_TELEMETRY=1  (optional)
```

Session resume in CI is possible (`kimi -p ... -S <id>` resumes; print mode re-forces auto), though a fresh session per job is the natural pattern. `docs/en/reference/kimi-command.md:117-133` is the canonical "script or CI" doc.

## 5. Auth & BYOK

- Config root: `~/.kimi-code/` (or `$KIMI_CODE_HOME`), holding `config.toml`, `mcp.json`, sessions, skills, OAuth credentials (`docs/en/configuration/data-locations.md:29-67`). `KIMI_CODE_HOME` relocates *all* runtime data — clean per-job isolation on ephemeral runners.
- **Providers** are TOML tables with a protocol `type`: `kimi` (OpenAI-compatible), `anthropic`, `openai` (Chat Completions; DeepSeek/Qwen/etc. work out of the box incl. `reasoning_content`), `openai_responses`, `google-genai`, `vertexai` (`docs/en/configuration/providers.md:7-16`). Each has overridable `base_url`.
- Credential resolution per provider: `api_key` (inline in TOML) **xor** `api_key_env` (name of a shell env var — deliberately re-read from the process env on every request, never cached) → else `[providers.<name>.env]` sub-table → else startup fails. **No implicit shell-env fallback** for credentials (`docs/en/configuration/overrides.md:34-53`).
- **Pure-env BYOK (the CI path): the `KIMI_MODEL_*` family synthesizes a temporary provider+model in memory when `KIMI_MODEL_NAME` is set** — `KIMI_MODEL_API_KEY` (required), `KIMI_MODEL_BASE_URL`, `KIMI_MODEL_PROVIDER_TYPE` (kimi/anthropic/openai), `KIMI_MODEL_MAX_CONTEXT_SIZE` (default 256 K), `KIMI_MODEL_CAPABILITIES` (e.g. `image_in,thinking`), plus display/output/reasoning overrides. Nothing is written to config.toml; `-m` still wins on top (`docs/en/configuration/env-vars.md:104-130`). Implementation: `packages/agent-core-v2/src/app/kosongConfig/envOverlay.ts` (alias `__kimi_env_model__`).
- `kimi provider` subcommand is explicitly "suitable for scripted deployments, CI initialization" — `provider catalog add <id> --api-key ... [--base-url ...]` imports from models.dev (built-in offline snapshot fallback), `provider add <registry-url>` bulk-imports custom registries (`docs/en/reference/kimi-command.md:323-404`).
- Managed OAuth (`/login`, `kimi login` RFC 8628 device-code) writes auto-refreshing credentials; `kimi login` exits 0/1 and prints URL+code to stderr (`docs/en/reference/kimi-command.md:138-146`). Human-in-browser → **not CI-suitable**; API-key providers are.
- MCP HTTP/SSE servers take static `headers` or `bearerTokenEnvVar` (env-var name) in `mcp.json` (`docs/en/customization/mcp.md`) — a GitHub token for a GitHub MCP server can be passed via secrets without landing in files.

## 6. Permissions / safety for unattended runs

- **Modes**: `manual` (ask by default), `yolo`/`-y` ("Ask When Needed": routine edits/commands run, risky actions/questions/plans still ask), `auto`/`--auto` ("Never Ask") (`src/cli/commands.ts:56-57`, `src/agent/permissionPolicy/types.ts:5`). **Print mode forces `auto`** (see §4) — headless runs never block on an approval.
- **Ordered policy chain** (first non-undefined result wins; `src/agent/permissionPolicy/permissionPolicyService.ts:39-55`): AskUserQuestion-deny-in-auto → user-configured deny → dangerous-command-ask (interactive only; skipped when `bootstrap.args.nonInteractive`) → auto-mode-approve → session-approval-history → user ask/allow → sensitive-file-access-ask → git-control-path-access-ask → yolo-mode-approve → default-tool-approve → git-cwd-write-approve → fallback-ask.
- **Static rules**: `[[permission.rules]]` in config.toml, first-match, `decision = allow|deny|ask`, patterns like `Read`, `Bash(rm -rf*)`, scopes `user/project/session-runtime/turn-override` (`docs/en/configuration/config-files.md:519-545`). Deny rules remain effective in print mode — the CI safety net. `[permission] dangerous_command_guard = false` (or `KIMI_CODE_DANGEROUS_COMMAND_GUARD=false`) turns off the built-in guard (root `AGENTS.md` note: it is never active in auto/non-interactive anyway).
- Dangerous-command guard parses bash with the in-repo pure-TS parser (`packages/tree-sitter-bash`, deterministic budget 500 ms / 10 k nodes) and matches privilege wrappers, `rm`/`mkfs`/shutdown-class commands, nested-shell and substitution forms (`src/agent/permissionPolicy/policies/dangerous-command-ask.ts:12-50`).
- **No OS-level sandbox**: the system prompt states "The environment is not a sandbox: your actions take effect on the user's system immediately" (`src/app/agentProfileCatalog/system.md:59`). Unattended CI safety must come from GitHub's ephemeral runner + permission deny rules + hooks, not from the engine.
- **Hooks**: `[[hooks]]` entries in config.toml run local shell commands on 19 events; exit code 2 blocks (PreToolUse runs *before* permission checks), 0 allows, anything else fail-opens. Events include UserPromptSubmit (can inject context), PreToolUse, PostToolUse, PermissionRequest/Result, Stop, TurnStarted, SessionStart/End, SubagentStart/Stop, TaskStarted, PreCompact/PostCompact, Notification, Interrupt, StopFailure, SessionHeartbeat, UserPromptQueued (`docs/en/customization/hooks.md:106-130`). Fail-open means hooks are aids, not the sole barrier (`hooks.md:22-24`).
- AskUserQuestion is auto-denied in auto mode (`policies/auto-mode-ask-user-question-deny.ts`), so an unattended run answers "no options" instead of hanging.

## 7. MCP, plugins, skills

- **MCP** (first-class): client support for stdio / HTTP / SSE servers via `mcp.json` at user level (`~/.kimi-code/mcp.json`) and project level (`.kimi-code/mcp.json`, name collisions resolved project-first). Per-server: `env`, `cwd`, `headers`, `bearerTokenEnvVar`, `enabled`, `startupTimeoutMs`, `toolTimeoutMs`, `enabledTools`/`disabledTools` allow/blocklists, experimental `deferred` on-demand loading via the `select_tools` tool. Tools appear alongside built-ins as `mcp__*` with no behavioral difference (`docs/en/customization/mcp.md`, profile tool lists include `'mcp__*'`, `profiles.ts:11-36`). Trust gate: headless needs `KIMI_CODE_TRUST_WORKSPACE=1` for project-level servers.
- **Plugins**: installable units (`kimi.plugin.json` manifest) that contribute skills, custom agents, session-loaded skills, system-prompt instructions, and MCP servers; installable from local dirs, zip URLs, or GitHub URLs (branch/tag/SHA-pinnable); marketplace at `plugins/marketplace.json` with official + curated tiers (`docs/en/customization/plugins.md`, `plugins/official/kimi-datasource/kimi.plugin.json`). Hidden `kimi __plugin_run_node <entry>` runs a plugin Node entry inside `KIMI_PLUGIN_ROOT` (realpath-checked) — plugins can ship arbitrary Node code (`src/cli/sub/plugin-run-node.ts:3-13`, `commands.ts:144-151`).
- **Skills**: SKILL.md (YAML frontmatter: name, description, type, whenToUse, disableModelInvocation, arguments) + optional supporting files; auto-loaded into context by description match, invocable via `/skill:<name>` or the `Skill` tool (inline type only, max nesting 3). Scan dirs: `$KIMI_CODE_HOME/skills/`, `~/.agents/skills/`, project `.kimi-code/skills/` and `.agents/skills/`; `--skills-dir` replaces discovery for one process; `extra_skill_dirs` persists (`docs/en/customization/skills.md:85-97`).
- **Agent Client Protocol**: `kimi acp` speaks ACP over stdio for Zed/JetBrains (`docs/en/reference/kimi-acp.md`) — an alternate headless control surface with IDE-style approvals, though print mode is the simpler CI path.
- **Server mode**: `kimi web` runs kap-server locally — REST+WS `/api/v1` with bearer-token auth (default `127.0.0.1:58627`), OpenAPI at `/openapi.json`; sessions, prompts, approvals, background tasks, skills/tools/MCP all scriptable over HTTP (`docs/en/reference/server-api.md:13-77`). A GitHub Action could drive multi-step workflows via this API, but `-p` is simpler for one-shot runs.

## 8. License

- **MIT** (`LICENSE`) — permissive, commercial use, reuse of code and patterns all fine; only requirement is retaining the copyright notice. The engine packages are in the same repo under the same license (each package.json declares `"license": "MIT"`). This is the most reuse-friendly license of the candidates.

## 9. Verdict: viability as the GitHub Action engine

**Yes — strong candidate, with a safety caveat.** Print mode (`kimi -p`) is a first-class, CI-documented headless runner with: JSONL streaming output, deterministic exit codes (0/1 + goal 3/6), never-block auto permissions with static deny rules, unbounded-by-default task timeouts, infinite-retry resilience, full env-var BYOK (`KIMI_MODEL_*` — zero config files to write), isolated state via `KIMI_CODE_HOME`, per-repo `AGENTS.md` discovery (fits the 22-repo org pattern), and MCP with `bearerTokenEnvVar` for a GitHub server. Runs on GitHub-hosted runners either via `actions/setup-node@v6` + `.nvmrc` (24.15.0 — this is exactly how the project's own CI runs, `.github/workflows/ci.yml:14-23`) or via the native SEA binary with no Node dependency at all.

**What to copy / adopt directly**
- The `-p` print-mode contract: forced-auto permissions, `stream-json` JSONL event stream, background-task policy (exit/drain/steer), exit-code discipline incl. the synchronous `process.exitCode = 1` before async logging (`apps/kimi-code/src/main.ts:249-262`) and the stdio-drain + force-exit backstop (`headless-exit.ts`).
- BYOK design: env-synthesized ephemeral provider (`envOverlay.ts`) + `api_key_env` no-caching semantics; MCP `bearerTokenEnvVar`.
- Ordered permission-policy chain with user deny-rules and print-mode auto; the tree-sitter-bash dangerous-command guard as a reference pattern.
- Subagent profile model (coder/explore/plan with per-profile tool lists, `mcp__*` inclusion, handoff discipline) and swarm fan-out for parallel repo triage across many repos.
- Print-mode config defaulting (only fill unset keys — `printDefaults.ts`).

**What to watch / avoid**
- **`-p` is full-auto**: no approval can ever interrupt a run, the dangerous-command guard is disabled under non-interactive bootstrap, and there is no sandbox (`system.md:59`). For "read code, comment" that's fine; for "commit fixes" the Action must supply its own gates (deny rules, hooks with exit 2, PR-scoped tokens, ephemeral runners).
- Node >= 24.15.0 with `engine-strict` — GitHub's default Node may lag; pin via setup-node or use the SEA binary.
- The engine (agent-core-v2, node-sdk, kosong, kap-server) is `private: true` — only the CLI is published. Programmatic embedding would require forking or asking Moonshot to publish the SDK; the CLI surface is the stable contract.
- Doc drift exists (e.g. `overrides.md:100` still shows `kimi --yolo -p`, which the CLI rejects) — validate against code, as done here.
- Version 2.x moves fast (2.1.0/2.1.1 within days); pin an exact CLI version in the Action and re-test on bumps.

## 10. Open questions

- Is Moonshot AI open to publishing `@moonshot-ai/kimi-code-sdk` (or blessing forks) for programmatic embedding? The SDK's `Session.prompt()` + approval handlers would beat shelling out for a long-lived Action agent.
- Would a `kimi acp` (ACP) or `kimi web` (REST/WS) integration give better multi-turn orchestration (e.g. responding to follow-up review comments) than repeated `kimi -p` invocations?
- How stable is `stream-json` output shape across versions for a parser in the Action? (Contract files under `packages/agent-core-v2/docs/wire-manifest.d.ts` and CLI surfaces are flagged as consumed-by-outsiders tripwires in root `AGENTS.md:112`, which is a good sign.)
- Any plans for an official `kimi` GitHub Action from Moonshot? The repo has none today (`.github/workflows/` is only their own CI/release).
- Cost/quota behavior under org-wide deployment: does Kimi managed auth support machine/service accounts, or must org repos use BYOK API keys per repo?
