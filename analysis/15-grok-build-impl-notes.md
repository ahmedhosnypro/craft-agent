# grok-build implementation deep-dive — design patterns for our Rust BYOK agent core

Date: 2026-10-10. Source tree: `/home/ahmed/Projects/grok-build` (xAI, Apache-2.0, ~84 crates).
Goal: lift **design patterns** (not code) for the storytold pure-Rust BYOK agent core.
All claims cite real file paths, verified by reading the source.

Layout map:
- `crates/codegen/xai-grok-shell` — the agent: `MvpAgent`, `SessionActor`, turn loop, ACP server.
- `crates/codegen/xai-grok-sampler` — sampling actor: HTTP, SSE, retry, per-request tasks.
- `crates/codegen/xai-grok-sampling-types` — pure data types (no I/O): conversation, requests, errors, Anthropic messages types.
- `crates/common/xai-tool-runtime` — the unified `Tool` trait + streaming primitives.
- `crates/common/xai-tool-protocol` — wire-neutral ids/capabilities (`ToolId`, `ToolCapabilities`).
- `crates/codegen/xai-grok-tools` — registry + built-in tool implementations (188 files).
- `crates/codegen/xai-grok-workspace` — permission engine (rules/modes/manager).
- `crates/codegen/xai-grok-sandbox` — Landlock/Seatbelt via `nono`, seccomp child-net block.
- `crates/codegen/xai-grok-pager` — TUI; also owns the headless (`grok -p`) driver (src/headless.rs).

## 1. Agent loop

### 1.1 Architecture: three nested layers
The "loop" is split across three layers (this is the pattern worth copying — each layer is independently testable):

1. **Agent layer** `MvpAgent` (`crates/codegen/xai-grok-shell/src/agent/mvp_agent/mod.rs:663`) — session registry, auth, config, subagent coordinator. NOT the loop itself; it spawns and registers sessions (`SessionSpawnOptions` at mod.rs:217).
2. **Session actor layer** `SessionActor` — one actor per session, spawned on a `LocalSet` thread via `spawn_session_on_thread` (imported at mvp_agent/mod.rs:70). Commands arrive as `SessionCommand` enum over mpsc (`src/session/commands.rs`).
3. **Turn layer** — `handle_turn_input` and the sampling loop, in `src/session/acp_session_impl/turn.rs` (4,466 lines).

### 1.2 Turn structure
`SessionActor::handle_turn_input` (turn.rs:486) → `handle_turn_input_inner` (turn.rs:503):
1. Slash/skill resolution (`SlashAuthority::HumanCatalog` at turn.rs:626) — human-intent prompts may resolve to builtin actions without sampling.
2. Prompt parsed into `{context, query, skill_information, images}` (`parse_prompt_with_skills`, turn.rs:984); images normalized, placeholders recovered.
3. User message pushed to chat state with a **persistence flush barrier** (`PersistenceMsg::FlushAndAck` + oneshot, turn.rs:1246-1267) — the turn does not sample until the user turn is durably on disk.
4. Then `loop { process_conversation_turn_with_recovery(...) }` (turn.rs:1312-1395) — the outer "round" loop. It re-runs the whole turn when: the stop-gate hook says KeepWorking (turn.rs:1388), the goal engine injects a continuation, or an interjection drained.

Inside, `process_conversation_turn_inner` (turn.rs:2605) runs the **inner sampling loop** per round (turn.rs:2680-3865):
- Prepare tool definitions (`prepare_tool_definitions_timed`, turn.rs:2647), build `ConversationRequest` from chat state (turn.rs:2860-2905).
- `run_turn_via_sampler` (sampler_turn.rs:1671) → one model call with retry classification.
- Match on `SamplerTurnOutcome`: `Response` / `RetryTransient` (sleep ladder + continue) / `CompactAndResubmit` (compaction ran; continue) / `RefreshAuthAndResubmit` (401 recovery paths).
- If `tool_calls.is_empty()` → turn ends: `TurnOutcome::Completed { tools_called, structured_output, stop: CompletedStop }` (turn.rs:3682-3687).
- Else `execute_tool_calls(...)` (turn.rs:3820) returns `ToolLoop`; then the **max-turns check** (turn.rs:3849-3853): `next_turn = tool_turn_count + 1; if next_turn > limit → TurnOutcome::MaxTurnsReached { limit }`.
- Loop continues (the tool results are already in chat state).

### 1.3 Outcome enums (sketch, from src/session/acp_session_impl/types.rs:84-110 and src/session/commands.rs:47)

```rust
// types.rs:84 — how a completed turn stopped
enum CompletedStop { EndTurn, MaxTokens, Refusal(String) }

// types.rs:92 — outcome of process_conversation_turn
enum TurnOutcome {
    Completed { tools_called: Vec<String>,
                structured_output: Option<Result<Value, String>>,
                stop: CompletedStop },
    Cancelled { category: Option<CancellationCategory>,
                context: Option<CancellationContext> },   // hook denied, permission rejected...
    MaxTurnsReached { limit: usize },
    StationarityEnded, // silent EndTurn after repeated identical tool calls
}

// commands.rs:47 — surfaced to the client (ACP PromptResponse _meta)
enum PromptCompletionKind {
    Completed, StationarityEnded,
    Cancelled { category, context },
    MaxTurnsReached { limit }, Rewound, RemovedFromQueue,
}
// Wire categories (commands.rs:73-90): "max_turns_reached",
// "action_stationarity", "HookDenied", "PermissionRejected",
// "PermissionCancelled", "MidTurnAbort" — camelCase in _meta.
```

### 1.4 Error/retry semantics (three distinct retry systems — keep them separate!)
1. **Sampler-internal** (L1, `xai-grok-sampler/src/retry.rs:80` `classify_error`): bounded retry of the HTTP call itself. 429 waits honor `Retry-After`; transport errors rebuild the reqwest client on first retry.
2. **Turn-level transient** (L2, sampler_turn.rs:38-79): sampler gave up → turn resubmits. Per-step cap `MAX_TRANSIENT_TURN_RETRIES = 3`, per-prompt cap 10, 10-minute episode window, backoff ladder [2s, 10s, 30s] jittered. Eligible: `IdleTimeout`, `Http`, retryable-5xx `Api` (never 429 — that is RateLimited, never Auth/Serialization/Empty).
3. **Auth 401 recovery** (L3, `AuthRetrySchedule`, acp_session_impl/auth_retry.rs): re-mint credential then resubmit; uncharged resubmits (request carried no credential, `SentCredential::Missing`) don't consume the budget; runaway guard stops when N rejections happen with no success between.
Plus **length salvage** (`length_salvage.rs`): on `StopReason::Length` inject a "continue" user message and re-sample, budget 5 per prompt (`MAX_OUTPUT_TOKEN_LIMIT_RETRIES`, sampler_turn.rs:14).

### 1.5 Doom-loop / stationarity detection (turn.rs:3866-3960)
`IdenticalToolCallRun` hashes each step's tool-call signature (args canonicalized: JSON keys recursively sorted, calls sorted; `step_signature` turn.rs:3925). Tight tier for read/plan tool kinds: nudge after 4 identical, stop after 8; loose tier nudge 8, stop 12; true-noop run (e.g. `true`) stops after 4. Cheapest high-value loop guard — port it.

### 1.6 Structured output without native schema support (turn.rs:14-31)
Backends without native JSON-schema enforcement (Anthropic Messages) get a synthetic `StructuredOutput` tool appended (turn.rs:2768-2776); the loop intercepts calls to it, validates args with `jsonschema` (validator compiled once per turn, turn.rs:2672), retries up to 3 times with the validation error fed back. `ApiBackend::supports_native_schema()` (sampling-types/src/types.rs:1059) decides which path.

## 2. Tool trait (`crates/common/xai-tool-runtime/src/tool.rs`)

### 2.1 The trait (tool.rs:37-113)
```rust
pub trait Tool: Send + Sync {
    type Args: Deserialize<'de> + JsonSchema + Send + 'static;   // schemars derives the schema
    type Output: Serialize + ToolOutput + Send + 'static;        // ToolOutput: model-facing blocks, default impl = {}
    fn id(&self) -> ToolId;                                      // stable routing identity
    fn description(&self, _ctx: &ListToolsContext) -> ToolDescription; // can be context-aware
    fn capabilities(&self) -> ToolCapabilities { default }        // read-only, concurrency, cancel
    fn has_dynamic_description(&self) -> bool { false }
    fn should_list(&self, _ctx: &ListToolsContext) -> bool { true }
    fn execute(&self, ctx: ToolCallContext, args: Self::Args)
        -> impl Future<Output = ToolStream<Self::Output>> + Send;  // RPITIT (async fn in trait), NOT boxed
    fn run(&self, ctx: ToolCallContext, args: Self::Args)
        -> impl Future<Output = Result<Self::Output, ToolError>> + Send; // blocking convenience
}
```
- Default `execute` wraps `run` into a one-item stream (`terminal_only`, tool.rs:207). A tool implementing neither fails loudly (`ToolError::not_implemented`, tool.rs:108).
- `run` is the 90% case — every built-in implements `run` (e.g. `ListDirTool` at xai-grok-tools/src/implementations/grok_build/list_dir/mod.rs:500).

### 2.2 Streaming invariant (tool.rs:115-133)
```rust
type ToolStream<T> = Pin<Box<dyn Stream<Item = ToolStreamItem<T>> + Send>>;
enum ToolStreamItem<T> { Progress(ToolProgress), Terminal(Result<T, ToolError>) }
// INVARIANT: any number of Progress, then exactly ONE Terminal, always last.
```
`ToolProgress` (tool.rs:140): `Text { text }` (stdout chunks), `Content { blocks }` (text+images), `Custom { subkind, payload }` (escape hatch with stable snake_case discriminator owned by the tool). Helpers to build streams: `terminal_only`, `deferred_terminal(fut)`, `with_progress(progress_stream, terminal_fut)` (tool.rs:207-243).

### 2.3 Type erasure: `ToolDyn` (tool.rs:322-418)
Registry stores `Arc<dyn ToolDyn>`. The blanket `impl<T: Tool> ToolDyn for T` (tool.rs:353) does the JSON boundary work once:
- deserialize `Value` → `T::Args` (parse error → `ToolError::invalid_arguments`, tool.rs:377),
- map the typed stream: `Progress` passes through; `Terminal(Ok(out))` serializes to `Value` AND captures `out.model_output()` blocks + optional `chat_completion_output()` into `TypedToolOutput { tool_id, value, model_output, chat_completion_output }` (tool.rs:262).
- MCP invariant: `model_output` is always non-empty (default extracts text blocks from the serialized JSON, `render::extract_content_blocks`).

### 2.4 Typed context via extension store (xai-tool-runtime/src/context.rs:17-77)
`ToolCallContext { call_id, extensions: TypedExtensions }` where `TypedExtensions` is a `HashMap<TypeId, Arc<dyn Any + Send + Sync>>` — tools pull exactly what they need: `Cwd`, `BehaviorVersion`, `TraceContext`, `Cancellation(CancellationToken)`, `SessionContext`, `WorkspaceViewerContext`, plus tool-defined `Params<P>`. This decouples tools from a mega-context struct.

### 2.5 Registration & schema generation (xai-grok-tools/src/registry/types.rs)
- `ToolRegistryBuilder::register_with_params::<T, P>()` (types.rs:532): instantiates `T::default()`, derives the fully-qualified name `"{namespace}:{id}"` (e.g. `"GrokBuild:read_file"`, types.rs:565), and captures: input schema via `generate_schema_cached::<T::Args>()` (types.rs:574; cached by `TypeId` at types.rs:2014), default params `P::default()`, `parse_input`, `output_converter`, `validate_params`/`apply_params` closures (params live in `Resources` as `Params<P>`).
- schemars is v1 (`Cargo.toml:251` `schemars = "1"`); tool Args structs derive `schemars::JsonSchema` and put per-field `#[schemars(description = "...")]` (list_dir/mod.rs:44-48).
- Out-of-tree extension: `register_tool_pack(pack: fn(&mut ToolRegistryBuilder))` global list (types.rs:30-40).
- `finalize(config, ctx) -> FinalizedToolset` (types.rs:936) validates tool requirements, resolves client-facing names/overrides/description templates, and builds `kind_to_name` maps. Tool descriptions are **templates** rendered per turn (TemplateRenderer), so the same tool can say "you are notified on completion" only when the client supports it.
- Runtime resources are shared via `bridge.read_resource::<T>()` / `update_resource(val)` (xai-grok-tools/src/bridge.rs:559-576).

### 2.6 How results stream back to the model
Tool execution in the session (`execute_tool_calls` → `dispatch_tool` in acp_session_impl/tool_calls.rs:335 and tool_dispatch.rs:16) consumes the `ToolDyn` stream:
- `Progress` items → client notifications (e.g. `bash_output_chunk`) — never enter the conversation.
- `Terminal(Ok)` → serialized into `ConversationItem::tool_result(call_id, message)` and pushed via `chat_state_handle.push_tool_result` (tool_calls.rs:2051) so the next request build includes it. Errors likewise become tool_result messages (tool_calls.rs:3245 `handle_tool_error`) — the loop never dies on a tool error; the model sees it.
- A rejected/cancelled call STILL gets a tool_result ("Tool execution cancelled because..."), tool_calls.rs:525-530 — keeps provider tool_use/tool_result pairs intact (Anthropic requirement).

## 3. Sampler / ApiBackend (xai-grok-sampler)

### 3.1 Protocol abstraction
One flat enum, not a trait — protocols differ in shape but share the full pipeline:
```rust
// xai-grok-sampling-types/src/types.rs:1046
pub enum ApiBackend { #[default] ChatCompletions, Responses, Messages }
impl ApiBackend {
    fn supports_native_schema(&self) -> bool;      // CC|Responses only
    fn forwards_prompt_cache_key(&self) -> bool;   // Responses only
    fn default_max_request_bytes(&self) -> NonZeroU64; // 50 MiB; Messages 30 MB
}
```
Dispatch per attempt in `actor/request_task.rs:521` `run_one_attempt`: match on `client.api_backend()` → call `conversation_stream` / `conversation_stream_responses` / `conversation_stream_messages` (client.rs:1967/2005/2082), then feed the raw chunk stream into the matching **L2 transform** (`stream/chat_completions.rs`, `stream/responses.rs`, `stream/messages.rs`) that normalizes to `SamplingEvent` (stream/mod.rs:1-13).

### 3.2 Request/response types (xai-grok-sampling-types/src/conversation.rs)
```rust
struct ConversationRequest {       // :623 — protocol-neutral ask
    items: Vec<ConversationItem>, // system/user/assistant/tool_result/notification-tagged
    tools: Vec<ToolSpec>,         // :485 { name, description, parameters: Value }
    hosted_tools: Vec<HostedTool>,// server-side tools (Responses API native types)
    /* tool_choice, max_output_tokens, temperature, top_p, json_schema,
       length_policy, reasoning_effort, prompt_cache_key, x_grok_* telemetry ids */
}
struct ConversationResponse {      // :822 — protocol-neutral answer
    items: Vec<ConversationItem>, stop_reason: Option<StopReason>, // :750 Stop/Length/ToolCalls/ContentFilter
    usage: Option<TokenUsage>,    // :775 prompt/completion/total/reasoning + cached_prompt_tokens (:783) + cache_creation (:785)
    cost_usd_ticks: Option<i64>,  // :833 server cost, 1e10 ticks = $1
    stop_message, message_id, raw_stop_reason, doom_loop_signals, ...
}
```
Conversions: `ConversationRequest → ChatCompletionRequest` (`request.into()`), `(&request) → rs::CreateResponse`, and `build_messages_request(&request)` for Anthropic (client.rs:1979, 2039, 2109). Auth per protocol: `AuthScheme::Bearer | XApiKey` (sampler/src/config.rs:20) — Messages uses `x-api-key` header (client.rs:525), others `Authorization: Bearer`.

### 3.3 SSE decode (client.rs:1155-1200)
`reqwest` `.bytes_stream()` → strip UTF-8 BOM on first chunk (eventsource-stream 0.2.3 bug workaround, client.rs:1158) → `.eventsource()` (eventsource-stream crate) → `scan` state machine: `data == "[DONE]"` ends the stream (chat_completions), first transport error is emitted then stream returns `None`, mid-stream JSON error envelopes parsed via `try_parse_stream_error(data)` (client.rs:1192). Per-chunk idle timeout wraps `stream.next()`: `tokio::time::timeout(idle_timeout, stream.next())` in each L2 transform (stream/chat_completions.rs:82, messages.rs:127) → `SamplingError::IdleTimeout`.

### 3.4 Retry / 429 classification (retry.rs:80-152 `classify_error`)
Order of checks: auth → encrypted-content → max_retries==0 fatal → payload-too-large/image errors → **RetryWithImageStrip** → retry-vetoed (`should_retry == Some(false)`, context-length, invalid-image) → fatal → doom-loop → **rate-limited** → retryable → fatal.
- `is_rate_limited()` (429) maps to its own kind: `SamplingErrorKind::RateLimited` (events.rs:267). 429 waits honor `Retry-After` header, capped at `MAX_RETRY_BACKOFF = 30s`, with jitter ±20%; threshold `RATE_LIMIT_RETRY_THRESHOLD = 2` attempts before surfacing to the session; `DEFAULT_MAX_RETRIES = 15`; exp backoff `2s << (n-1)` capped 30s (retry.rs:39-64). Env override `GROK_MAX_RETRIES` (retry.rs:22).
- `RetryDecision` variants: `Retry{backoff}`, `RetryWithBackoff{is_rate_limited}`, `RetryWithImageStrip`, `RetryWithClientRebuild{backoff}` (first Http retry rebuilds the reqwest client), `EmitToSession(err)` (auth — the turn layer owns re-minting), `Fatal`.
- `retry_only_before_output: bool` (config.rs:206): once any output token was observed, transport retries stop (request_task.rs:147) — prevents double-billing partial responses.
- Session-level 429 budget: `RateLimitWaitBudget` (acp_session.rs:858 area; `resolve_subagent_rate_limit_max_attempts` mvp_agent/mod.rs:1101) lets the turn wait out 429s across requests in unattended runs — env/config/remote/default cascade.

### 3.5 Idle timeout
Per-chunk, not per-request: default 300s (`DEFAULT_IDLE_TIMEOUT_SECS`, request_task.rs:41), config `SamplerConfig::idle_timeout_secs` (config.rs:78), model-level override `[model.*] inference_idle_timeout_secs` floor 10s default 600 (mvp_agent/mod.rs:1088-1099). NOT retryable inside the sampler (error.rs comment at :157-159) — the turn layer decides.

### 3.6 Actor model (actor/mod.rs:29-60, handle.rs)
`SamplerActor::spawn(config, retry_policy, event_tx) -> SamplerHandle` — actor task is a single-threaded mpsc command loop; each request is a `tokio::spawn`ed task tracked in a `JoinSet` (actor/mod.rs:13-16). `SamplerHandle` is a cloneable `UnboundedSender<SamplerCommand>`; `submit_and_collect_with_metadata` returns a `CollectedSamplingResult` (handle.rs:27) with `result`, `terminal_event_queued`, doom-loop metadata. Cancellation via `CancellationToken` checked with `biased` select inside every attempt loop (request_task.rs:507-515). This is a clean pattern for a library core: the loop never holds the HTTP future directly.

### 3.7 Assessment: adding a 4th protocol (Google Gemini generateContent)
**Clean — 3 touch points, no trait surgery:**
1. Add `ApiBackend::Gemini` variant (types.rs:1046) + its `supports_native_schema` (yes — responseSchema), `default_max_request_bytes`, etc.
2. Add `build_gemini_request(&ConversationRequest)` + a `messages.rs`-style wire-types module in `xai-grok-sampling-types` (mirror `messages.rs:271` `MessageStreamEvent` enum with Gemini's streamGenerateContent SSE chunks: `contentBlockDelta`-ish candidates/parts + `usageMetadata`).
3. Add `conversation_stream_gemini` on `SamplingClient` + `stream/gemini.rs` L2 transform + one match arm in `run_one_attempt` (request_task.rs:521).
Because the dispatch is an enum match over `conversation_stream*` methods returning `BoxStream<Result<RawChunk>>`, and all retry/error classification is protocol-neutral (`SamplingError` is shared), Gemini slots in with zero changes to the turn loop or retry ladder. Auth: Gemini uses `x-goog-api-key` header or `?key=` query param — the `extra_headers`/`query_params` fields on `SamplerConfig` (config.rs:55,61) already support both.

## 4. BYOK config

### 4.1 TOML schema (all in `crates/codegen/xai-grok-shell/src/agent/config.rs` + `model_providers.rs`)
```toml
[model.<id>]                # config.rs:3733 ModelEntryConfig
model = "claude-sonnet-4"   # routing slug sent to the API
base_url = "https://api.anthropic.com"    # required
api_key = "sk-..."          # static key (or env_key)
env_key = "ANTHROPIC_API_KEY"              # string OR array (config.rs:56 EnvKeys, serde untagged)
api_backend = "messages"    # chat_completions (default) | responses | messages (config.rs:3777)
auth_scheme = "x_api_key"  # bearer (default) | x_api_key
context_window = 200000     # REQUIRED NonZeroU64 for config-file models (config.rs:3816)
max_completion_tokens, temperature, top_p, reasoning_effort, extra_headers (IndexMap),
max_request_bytes, inference_idle_timeout_secs, auto_compact_threshold_percent,
model_provider = "myproxy"  # inherit from a provider table
auth_provider = "vault"    # credential helper name

[model_providers.<id>]      # model_providers.rs:10 ModelProviderConfig
base_url, api_base_url, env_key, api_key, api_backend, extra_headers, query_params,
env_http_headers,   # header-name → env-var, resolved at client build, never persisted (:27)
auth_provider, auth (inline AuthProviderConfig), context_window, max_request_bytes

[auth_provider.<name>]      # xai-grok-config-types/src/auth_provider.rs:9
command = "vault kv get ..."   # required; shell if no args, direct exec if args set
args = ["get", "secret"]       # optional Vec
token_ttl_secs, timeout_secs (default 30, clamp 1..=600), cwd (~ expands)
```

### 4.2 Credential resolution order (config.rs:4723 `resolve_credentials`)
```
1. model.api_key (non-blank)                    → AuthType::ApiKey
2. model.env_key (first SET, non-blank env var) → ApiKey     [EnvKeys::resolve_value, config.rs:105]
3. model.auth_provider cached token (subprocess minted)        → ApiKey
4. session token (only if AuthBackend::may_receive_session(base_url))
5. XAI_API_KEY env var (with api_base_url override)            → ApiKey
6. none — warn if env_key configured but unset (config.rs:4771)
```
`EnvKeys` (config.rs:56-116): `#[serde(untagged)] enum EnvKeys { One(String), Many(Vec<String>) }` — TOML accepts `env_key = "FOO"` or `env_key = ["FOO", "BAR"]`; `resolve_value_with` walks names in order, first set non-blank wins; semantic `PartialEq` compares name lists so `One("X") == Many(["X"])`.
Conflict warnings at parse time (model_providers.rs:104-135): static `api_key` shadows an auth helper (helper never runs); `env_key` shadows helper only when the var resolves; inline `auth` shadowed by `auth_provider` reference. `serde_ignored` reports unknown fields as per-key warnings (model_providers.rs:75) — great DX pattern for config files.

### 4.3 What our core should copy
- `[model.<id>]` + `[model_providers.<id>]` two-level inheritance (provider fields inherited by models) with per-field override.
- `env_key` string-or-array via untagged enum.
- `extra_headers` + `env_http_headers` (secrets never persisted in config; resolved at client build).
- `auth_provider` command minting with ttl/timeout/cwd for CI OIDC helpers (e.g. `gh auth token`).
- warnings-not-errors config parsing (typo'd keys surface, don't crash).

## 5. Permissions / sandbox

### 5.1 Permission engine (crates/codegen/xai-grok-workspace/src/permission/)
- Modes (`rules.rs:11` `DefaultPermissionMode`): `default | acceptEdits | plan | auto | dontAsk | bypassPermissions` ("yolo" = `SetYoloMode(true)` command, types.rs:238 `PermissionCommand`).
- Rule grammar (`rules.rs:107` `parse_permission_rule`): `"Bash(npm test:*)"` / `"Read(src/**)"` / `"WebFetch(domain:example.com)"` / `"mcp__server__tool"` (rewritten to `<server>__<tool>`) / bare tool name = wildcard. Parsed into `PermissionRule { action: Allow|Deny|Ask (types.rs:412), tool: ToolFilter (Any|Bash|Edit|Read|Grep|Mcp|WebFetch|WebSearch, types.rs:421), pattern, pattern_mode: Glob|Domain (types.rs:405) }`. `Bash(cmd:*)` prefix semantics special-cased (rules.rs:168).
- Decision flow (types.rs:135): `Allow | Ask | FollowupMessage(String) | Reject(String) | PolicyDeny(String) | Cancelled` — the turn loop maps `PermissionReject` → `TurnOutcome::Cancelled{category: PermissionRejected}` (turn.rs:3824).
- Access classification: `From<&ToolInput> for AccessKind` is an allowlist of read-only tools — every unclassified tool prompts as an edit (types.rs:225 comment).

### 5.2 Sandbox layers (crates/codegen/xai-grok-sandbox/src/lib.rs:8-13)
- **Process sandbox**: applied ONCE at startup via `nono` 0.53.0 (Cargo.toml:35, pinned exact because macOS Seatbelt deny-precedence depends on nono's rule emission order) → Landlock (Linux) / Seatbelt (macOS). `SandboxManager::new(ProfileName::Workspace, workspace)` + `.apply()` + `.install()` (lib.rs doc example :16-21). Profiles: `Off | Workspace | Devbox | Custom(name)` with `extends = "devbox"` config.
- **Process network stays OPEN** (the agent itself needs the LLM API); **child processes get their network blocked** by a raw seccomp BPF filter installed per-spawn on Linux: `PR_SET_NO_NEW_PRIVS` + `SECCOMP_SET_MODE_FILTER` + `TSYNC` (child_net.rs:142-171). Keyed on the config (fail-closed), NOT on Landlock success, so degraded kernels still get child-net blocking (lib.rs:83-92).
- **Deny globs** are expanded to concrete paths at launch, best-effort on Linux, hard-fail on expansion error (lib.rs:556-569).
- **Hook write-deny**: even inside the sandbox, files that would trigger hooks cannot be written (hook_write_deny.rs) — closes the "agent rewrites its own permission config" hole.
- **Auto-allow bash when sandbox active**: `should_auto_allow_bash()` (lib.rs:110) — sandbox enforcement substitutes for prompting. This is exactly the composition our GitHub Action needs: unattended run = sandbox (or none) + `--permission-mode bypassPermissions` OR `dontAsk` + allow/deny rules.

### 5.3 CI composition recipe (derived from the above)
For an unattended GitHub Action: `grok -p "<task>" --permission-mode default/acceptEdits` + allow rules for read-only + the runner workspace is already ephemeral, so a Landlock profile scoped to `$GITHUB_WORKSPACE` + seccomp child-net-block (except when a tool genuinely needs network — policy-gated via `ChildNetworkPolicy`, network_policy.rs) gives defense without prompting. `dontAsk` maps promptable actions to `Decision::PolicyDeny` so the bot fails closed instead of hanging on a prompt — the correct unattended default.

## 6. Headless contract (`grok -p`)

### 6.1 Surface (crates/codegen/xai-grok-pager/src/app/cli.rs:460-690, headless/cli.rs)
- `-p`/`--print`/`--single` = one prompt, run to completion, exit (cli.rs:474). Prompt also from `--prompt-json` (ACP content blocks) or `--prompt-file` (.json = blocks, else text; headless/cli.rs:46-64).
- `--output-format` (`OutputFormat`, headless/cli.rs:9): `plain` (default) | `json` | `streaming-json` (NDJSON, one ACP session update per line — the agent's native format) | `streaming-messages-json` (NDJSON in Anthropic Messages wire format).
- `--tools <comma-list>` allowlist and `--disallowed-tools` (cli.rs:658-663); `--max-turns <u32>` (cli.rs:666-670); `--json-schema` (single JSON Schema object, headless/cli.rs:16-24) → validated final output delivered in the prompt-response `_meta.structuredOutput` (mvp_agent/mod.rs:459-462).
- `--yolo` (bypass), `--exit-after-first-turn`, `--background-wait-timeout-secs` (headless only; default waits for pending background bash/subagents before exiting, headless.rs:87, 1203-1231).

### 6.2 Result envelope and usage ticks
Final `json`/`plain` result carries (extensions/notification.rs:324-402 `project_result_usage` + :415 `attach_result_usage_fail_closed`):
```json
{
  "usage": { "input_tokens": <uncached = full − cache_read − cache_creation>,
             "cache_read_input_tokens": ..., "cache_creation_input_tokens": ...,
             "output_tokens": ..., "reasoning_tokens": ..., "total_tokens": ... },
  "num_turns": <same unit as --max-turns>,
  "usage_is_incomplete": true?,        // fail-closed: parse failure sets this, never silently omits
  "total_cost_usd": <float>, "total_cost_usd_ticks": <i64>,   // ONLY when complete & not partial
  "modelUsage": { "<model>": { inputTokens, outputTokens, cacheReadInputTokens,
                               cacheCreationInputTokens, modelCalls, costUSD? } },
  "structuredOutput": ..., "structuredOutputError": ...
}
```
Costs are carried as **integer USD ticks (1e10 = $1)** through the whole pipeline (`USD_TICKS_PER_USD` notification.rs:309; `ConversationResponse::cost_usd_ticks` conversation.rs:833) and only converted to float at the headless boundary — exact-sum reconciliation. Costs are scrubbed whenever any call lacked cost (`cost_is_partial`) or usage is incomplete.

### 6.3 Exit codes (headless.rs:1370-1428)
- Success → 0. Errors → non-zero via the returned `anyhow::Error` (main.rs wrapper).
- **Max-turns**: the response `_meta.cancellationCategory == "max_turns_reached"` (commands.rs:73) is detected at headless.rs:1389-1396 → emits terminal events then `Err("max turns reached")` → non-zero exit. Headless clients drive max-turns exit-code policy off this category string.
- Rate-limited final failure uses a dedicated wire error code (`RATE_LIMITED_ERROR_CODE`, headless.rs:1407) with a sanitized user message.
- Errors still attach the prompt usage spend (`prompt_usage_from_error`, headless.rs:1422) so a failed run reports its cost.
- Signals during the turn force-exit with the signal code (headless.rs:1361-1367); stdout write errors surface as exit failures (headless.rs:111).

### 6.4 What our CLI's output contract should look like (recommendation)
Copy: (1) `--output-format json|streaming-json` where streaming-json = NDJSON events with a typed `type` per line and one terminal `result` line; (2) result envelope with `usage` (token buckets incl. cache split), `num_turns`, integer `total_cost_usd_ticks`, `usage_is_incomplete` fail-closed flag, per-model `modelUsage`; (3) `cancellation_category` strings on the terminal event (`max_turns_reached`, `permission_rejected`, ...) mapped to distinct exit codes; (4) `--tools` allowlist + `--max-turns`; (5) errors never swallow usage. Skip: the Anthropic-wire streaming format and ACP as the only event schema — define our own small event enum but keep it JSON-lines.

## 7. What to AVOID / not port

1. **Leader IPC** (`xai-grok-shell/src/leader/`): single-leader-per-machine Unix-socket multiplexer (leader/mod.rs:3-40) with roster merge, client capability arbitration, config-file watchers. Massive complexity; a per-Action CLI needs none of it. The `MvpAgent` struct itself carries ~80 fields largely to serve leader/TUI/dashboard state (mvp_agent/mod.rs:663-883) — do NOT copy that struct; extract only session-spawn essentials.
2. **TUI / pager** (`xai-grok-pager`, ratatui-based) and the ACP (agent-client-protocol) client-server split: our Action CLI should be a single process, not a client-server pair. Keep the *event* discipline ACP enforces (typed SessionUpdate notifications) but drop the wire protocol.
3. **Marketplace / plugins / skills / hooks / announcements / remote settings / telemetry upload** (xai-grok-plugin-marketplace, xai-grok-hooks, xai-grok-announcements, upload::trace GCS exporters, subscription/paywall gates — mvp_agent/mod.rs:107-165 tier/JWT logic): all product-ecosystem machinery. Out of scope for a BYOK core.
4. **Auto-updater** (`xai-grok-update/src/auto_update.rs`), **memory v2 dream system** (session/memory*), **worktrees** (xai-fast-worktree), **session registry/dashboard**, **voice**, **workflow engine**, **goal orchestrator** (session/goal_*.rs — 10+ files): none needed for a PR-summarizer bot core.
5. **async-openai fork coupling**: root `Cargo.toml:3-4` has `[patch.crates-io] async-openai = { git = "github.com/our-forks/async-openai", rev = "95b52eb..." }` and `Cargo.toml:132` pins `async-openai = "0.33.0"` with the `responses` feature. BUT the coupling is shallow: the sampler does NOT use async-openai's `Client`/transport at all — it hand-rolls reqwest + eventsource-stream (client.rs:17, 27) and uses async-openai **only for types**: `pub use async_openai::types::responses as rs` (sampling-types/src/lib.rs:36) for Responses API request/stream-event structs, plus hand-written `messages.rs` (Anthropic) and `ChatCompletion*` types (sampling-types/src/types.rs). Comments note re-parsing for "fields async_openai's types omit" (client.rs:132) and splicing "xAI-specific fields not in async-openai's CreateResponse type" (client.rs:1458) — i.e. the fork exists to un-cripple the types. **A small core can avoid it entirely**: hand-write the ~10 request structs + stream-event enums per protocol (they did exactly that for Anthropic in `messages.rs`, 400-ish lines). reqwest + eventsource-stream + serde is the whole transport.
6. **LocalRef raw-pointer `'static` hack** (mvp_agent/mod.rs:14-33) and the `LocalSet` single-thread actor design: works but is `!Send` gymnastics; our core should stay plain tokio multi-thread with `Arc` + mpsc.
7. **Doom-loop server signaling** (`x-grok-doom-loop-check` header, doom_loop.rs): xAI-proxy-specific; keep only the client-side identical-call detection from 1.5.

## PORT THIS — checklist for our core crate

| # | Pattern | grok-build source | Our core crate needs |
|---|---------|-------------------|----------------------|
| 1 | Protocol-neutral `ConversationRequest`/`ConversationResponse` with per-protocol conversion fns | sampling-types/src/conversation.rs:623,822 | `core::types` — same structs, minus `hosted_tools`/`x_grok_*` telemetry fields |
| 2 | Flat `ApiBackend` enum + per-protocol `conversation_stream_*` + L2 transform to shared `SamplingEvent` | types.rs:1046; request_task.rs:521; stream/*.rs | 4 variants: `Anthropic`, `OpenAi` (base_url overridable), `OpenAiCompatible`?→ same as OpenAi w/ base_url, `Gemini`. Gemini fits cleanly (see 3.7) |
| 3 | Hand-rolled SSE: reqwest bytes_stream + eventsource + `[DONE]` scan + BOM strip | client.rs:1155-1200 | copy as-is; add Gemini SSE (data: JSON per chunk, no [DONE]) |
| 4 | Error taxonomy: `SamplingError` with `Api{status, retry_after_secs, should_retry, error_code}` + `SentCredential` provenance | sampling-types/src/error.rs:134-216 | same; `should_retry` header may go, keep status+retry_after |
| 5 | Two-tier retry: sampler-internal classify (429 Retry-After, 15 max, image-strip, client-rebuild) + turn-level transient ladder (3/10, 2-10-30s) + auth resubmit | retry.rs:80-152; sampler_turn.rs:38-79 | keep both tiers; drop doom/length-salvage initially, keep length-continue |

| # | Pattern | grok-build source | Our core crate needs |
|---|---------|-------------------|----------------------|
| 6 | Per-chunk idle timeout (300s default) wrapping `stream.next()` in each L2 transform | request_task.rs:41; stream/chat_completions.rs:82 | copy; make configurable per provider (Anthropic long thinking needs it) |
| 7 | Sampler as actor: mpsc commands + JoinSet per-request tasks + CancellationToken | actor/mod.rs:29-60; handle.rs:31 | copy — lets a library consumer drive/cancel cleanly |
| 8 | `Tool` trait: typed `Args: JsonSchema` / `Output: ToolOutput`, `run` default-blocking, `execute` streaming, RPITIT not boxed | xai-tool-runtime/src/tool.rs:37-113 | copy nearly verbatim (schemars v1) |
| 9 | Stream invariant `Progress* → exactly one Terminal`; helpers terminal_only/deferred_terminal/with_progress | tool.rs:115-243 | copy |
| 10 | `ToolDyn` erasure + `TypedToolOutput{value, model_output}` w/ non-empty model_output invariant | tool.rs:262-418 | copy; model_output default = JSON-text block |
| 11 | `TypedExtensions` TypeId-keyed context (Cwd, Cancellation, Params<P>) | xai-tool-runtime/src/context.rs:17-77 | copy — tiny and decoupling |
| 12 | Registry: `register_with_params::<T,P>` with cached schemars schema + `finalize` w/ requirements validation | xai-grok-tools/src/registry/types.rs:496-640, 936 | copy shape; skip description templates + tool packs |
| 13 | Tool errors become tool_result messages, never kill the turn; cancelled calls still pair | tool_calls.rs:3245, 525-530 | copy — provider-pairing invariant |
| 14 | Max-turns counted per tool-execution round; terminal category string `max_turns_reached` drives exit code | turn.rs:3849; commands.rs:73; headless.rs:1389 | copy |
| 15 | Identical-call stationarity guard (canonical JSON args, sorted step signature, nudge-then-stop) | turn.rs:3866-3960 | copy — cheap doom-loop protection |
| 16 | StructuredOutput synthetic tool for schema-less backends (validate with jsonschema, ≤3 retries) | turn.rs:14-31, 3687 | copy for Anthropic; native responseSchema for Gemini, json_schema for OpenAI |
| 17 | `[model.<id>]` + `[model_providers.<id>]` TOML w/ inheritance, `env_key` str-or-array, `extra_headers`, `env_http_headers` | config.rs:3733; model_providers.rs:10; EnvKeys config.rs:56 | copy schema; add `[providers.anthropic] fixed-endpoint` convenience |
| 18 | Credential resolution order (api_key > env_key > auth_provider > session > global env) + parse-time conflict warnings via serde_ignored | config.rs:4723; model_providers.rs:75-135 | copy |
| 19 | `[auth_provider]` command minting (ttl/timeout/cwd, direct-exec vs shell) | config-types/src/auth_provider.rs:9 | copy — CI OIDC/gh-token helpers |
| 20 | USD cost as integer ticks (1e10/$) end-to-end; scrub when partial; headless projects float+ticks | notification.rs:228-313; conversation.rs:833 | copy — exactness matters for daily-cron spend reports |
| 21 | Headless result envelope: usage buckets (cache split), num_turns, usage_is_incomplete fail-closed, modelUsage | notification.rs:324-402 | copy; this is the Action's machine contract |
| 22 | `--tools` allowlist + `--max-turns` + output-format json/streaming-json on the CLI | pager/src/app/cli.rs:658-670; headless/cli.rs:9 | copy |
| 23 | Permission rules grammar `Tool(pattern)` glob/domain + Allow/Deny/Ask + modes incl. dontAsk (fail-closed unattended) | workspace/src/permission/rules.rs:107; types.rs:387,412 | copy the subset: default/acceptEdits/dontAsk/bypass + allow/deny globs |
| 24 | Sandbox: nono Landlock/Seatbelt once at startup + per-spawn seccomp child-net block keyed on config (fail-closed) + sandbox⇒auto-allow-bash | sandbox/src/lib.rs:8-13, 83-92; child_net.rs:142 | optional 2nd phase; Linux-only Landlock fine for GH runners; seccomp child-net is high value |
| 25 | Self-skipping real integration tests? | — (grok-build has no BYOK-key test pattern; sampler tests are unit/mocked: client.rs:2958) | NOT from grok-build: design our own — read keys from gitignored `.env`, `if env var absent → eprintln + return Ok(())` |

**Do NOT port**: leader IPC, ACP wire, pager/TUI, marketplace/plugins/skills/hooks, remote settings/telemetry-upload/paywall, auto-updater, memory/worktrees/goals/workflows, async-openai fork (hand-write types), LocalSet/LocalRef pointer hack, server doom-loop headers.
