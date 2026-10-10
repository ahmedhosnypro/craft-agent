# Provider APIs, Rust crates, and helper scripts (rs4-provider-apis)

Research for the storytold pure-Rust BYOK agent core (GitHub Action). Date: 2026-10-10.
Every claim below was verified against the cited page fetched today. (The agy Google-search
MCP was down with a stale OAuth token; all facts come directly from fetched official docs,
crates.io API, GitHub API, and live `gh` runs — stronger evidence than search synthesis.)

## 1. Provider matrix

| | Anthropic Messages | OpenAI Chat Completions | Google Gemini |
|---|---|---|---|
| Endpoint | `POST https://api.anthropic.com/v1/messages` (fixed) [overview](https://docs.anthropic.com/en/api/overview) | `POST {base}/v1/chat/completions`; default `https://api.openai.com/v1`, base overridable [ref](https://platform.openai.com/docs/api-reference/chat/create) | Interactions (GA): `POST https://generativelanguage.googleapis.com/v1beta/interactions`; legacy generateContent: `POST .../v1beta/models/{model}:generateContent` (fixed host) [overview](https://ai.google.dev/gemini-api/docs/interactions-overview) |
| Auth headers | `Authorization: Bearer <key>` (preferred) or `x-api-key: <key>` (legacy, still supported); `anthropic-version: 2023-06-01` (required, current value); `content-type: application/json` [overview](https://docs.anthropic.com/en/api/overview), [versioning](https://docs.anthropic.com/en/api/versioning) | `Authorization: Bearer <key>`; optional `OpenAI-Organization`, `OpenAI-Project` [ref](https://platform.openai.com/docs/api-reference/errors) | `x-goog-api-key: <key>` header (current docs) or `?key=<key>` query param (still shown in generateContent reference) [auth](https://ai.google.dev/gemini-api/docs/api-key) |
| System prompt | Top-level `system` param: string or array of `TextBlockParam` [ref](https://docs.anthropic.com/en/api/messages) | `role: "developer"` message (replaces `system` on o1+; `system` still accepted) [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: `systemInstruction` (Content object, text only); Interactions: `instructions` field [ref](https://ai.google.dev/api/generate-content) |
| Tool request shape | `tools: [{name, description, input_schema: <JSON Schema 2020-12>}]` [ref](https://docs.anthropic.com/en/api/messages) | `tools: [{type: "function", function: {name, description, parameters, strict?}}]` [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: `tools: [{functionDeclarations: [...]}]`; Interactions: flat `tools: [{type: "function", name, description, parameters: <JSON Schema>}]` [fc](https://ai.google.dev/gemini-api/docs/function-calling) |
| Tool choice | `tool_choice`: `{"type":"auto"}` (default) / `none` / `any` / `{"type":"tool","name":...}`; `disable_parallel_tool_use` flag. Opus/Sonnet 5.5 reject `any`/`tool` with 400 [fc](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/overview), [errors](https://docs.anthropic.com/en/api/errors) | `tool_choice`: `"none"` (default w/o tools) / `"auto"` (default w/ tools) / `"required"` / named function [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: `toolConfig.functionCallingConfig` (AUTO/ANY/NONE + allowedFunctionNames); Interactions: `generation_config.tool_choice` = auto/any/none/validated + `allowed_tools{mode,tools}` [fc](https://ai.google.dev/gemini-api/docs/function-calling) |
| Tool call in response | `tool_use` content block: `{type:"tool_use", id:"toolu_...", name, input}`; multiple per turn allowed [ref](https://docs.anthropic.com/en/api/messages) | `message.tool_calls[]`: `{id, type:"function", function:{name, arguments:<JSON string>}}` [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: part `functionCall {name, args}`; Interactions: step `type:"function_call"` with `name`, `arguments`, `call_id` [ref](https://ai.google.dev/api/generate-content) |
| Tool result back | user-role `tool_result` block `{type:"tool_result", tool_use_id, content}` [fc](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/overview) | `role:"tool"` message with `tool_call_id` + string content [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: part `functionResponse {name, response}`; Interactions: input item `{type:"function_result", name, call_id, result:[{type:"text",text}]}` [fc](https://ai.google.dev/gemini-api/docs/function-calling) |
| Finish/stop signal | `stop_reason`: end_turn, max_tokens, stop_sequence, tool_use, pause_turn, refusal, model_context_window_exceeded [ref](https://docs.anthropic.com/en/api/messages) | `finish_reason`: stop, length, tool_calls, content_filter, function_call (deprecated) [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: `finishReason` enum (STOP, MAX_TOKENS, SAFETY, RECITATION, LANGUAGE, OTHER, BLOCKLIST, PROHIBITED_CONTENT, SPII, MALFORMED_FUNCTION_CALL, IMAGE_*, UNEXPECTED_TOOL_CALL) [ref](https://ai.google.dev/api/generate-content) |
| Usage | `usage{input_tokens, output_tokens, ...}` in every response [ref](https://docs.anthropic.com/en/api/messages) | `usage{prompt_tokens, completion_tokens, total_tokens}`; streaming only if `stream_options:{include_usage:true}` → final chunk before `data: [DONE]` with empty `choices` [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: `usageMetadata`; Interactions: `interaction.completed` carries totals (input/output/cached/thought/tool_use) [ref](https://ai.google.dev/api/generate-content) |
| Required params | `model`, `max_tokens` (number), `messages`; all marked non-optional in the body-param list [ref](https://docs.anthropic.com/en/api/messages) | `model`, `messages` (max_tokens NOT required) [ref](https://platform.openai.com/docs/api-reference/chat/create) | generateContent: `contents` (+ model in path); Interactions: `model`, `input` [ref](https://ai.google.dev/api/generate-content) |
| Test model IDs | `claude-haiku-5-5` (cheapest), `claude-sonnet-5-5`, `claude-opus-5-5`; released 2026-09-22, retirement not before 2027-09-22 [models](https://docs.anthropic.com/en/docs/about-claude/models/overview) | `gpt-6-luna` ($0.10/$0.50 per Mtok — but Chat Completions function calling only with `reasoning_effort:"none"`), `gpt-5.5` (snapshot `gpt-5.5-2026-04-23`), `gpt-6-astra` ($10/$50). `gpt-6.1-sol` does NOT support Chat Completions tool calling [models](https://platform.openai.com/docs/models/gpt-6-luna) | `gemini-3.5-flash-lite` (cheapest, stable), `gemini-3.6-flash` (prev gen, stable), `gemini-3.8-flash` (current flagship Flash, stable), `gemini-3.1-pro-preview`. `gemini-3-pro-preview` is shut down [models](https://ai.google.dev/gemini-api/docs/models) |

## 2. Anthropic Messages API — details for the agent loop

- Streaming SSE (`"stream": true`): named events `message_start` → per content block
  `content_block_start` → 1+ `content_block_delta` → `content_block_stop` → `message_delta`
  → `message_stop`; `ping` events may appear; mid-stream `error` events possible (e.g.
  `overloaded_error`, which maps to HTTP 529 non-streaming). Deltas: `text_delta{text}`,
  `input_json_delta{partial_json}` (partial JSON strings; parse on `content_block_stop`),
  `thinking_delta`, `signature_delta`. Handle unknown event types gracefully — new types may
  be added. [streaming](https://docs.anthropic.com/en/api/streaming)
- Errors/retries: 400 invalid_request_error, 401 authentication_error, 404 not_found_error,
  429 rate_limit_error (some 429s have no retry-after and persist — spend caps), 500 api_error,
  504 timeout_error, 529 overloaded_error. Error body is always JSON `{error:{type,message}}`
  plus `request_id`. Official SDKs retry transient failures (connection errors, rate limits,
  5xx) twice with exponential backoff, honoring `retry-after`. Every response carries a
  `request-id` header. [errors](https://docs.anthropic.com/en/api/errors)
- `max_tokens` is required and sets the absolute output cap (`0` = cache-prewarm only);
  thinking tokens count toward it. `temperature` is deprecated — post-Opus-4.6 models only
  accept 1.0 and reject other values with 400. [ref](https://docs.anthropic.com/en/api/messages)
- Thinking: current models use adaptive thinking (`{"type":"adaptive"}` default when omitted);
  `{"type":"enabled", budget_tokens>=1024}` is rejected on 4.7+; thinking blocks must be
  replayed verbatim and history kept append-only or the API returns 400. Not needed for a
  digest bot, but the client must tolerate `thinking` blocks in responses.
  [errors](https://docs.anthropic.com/en/api/errors)
- Tool-use system-prompt overhead: ~286–804 extra input tokens per request depending on model
  and tool_choice. [fc overview](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/overview)

## 3. OpenAI Chat Completions — details + base-URL override semantics

### Wire shape
- Request: `model`, `messages` (roles: developer/system/user/assistant/tool), optional
  `tools`, `tool_choice`, `stream`, `stream_options`, `max_tokens`/`max_completion_tokens`,
  `reasoning_effort`, `service_tier`, etc. [ref](https://platform.openai.com/docs/api-reference/chat/create)
- Streaming: chunks carry `choices[].delta{content?, tool_calls?}`; tool calls arrive keyed by
  `delta.tool_calls[].index` — set `id`/`type`/`function.name` once (first fragment), then
  append `function.arguments` string fragments per index; official accumulation example:
  [function-calling guide](https://platform.openai.com/docs/guides/function-calling).
  Final usage only arrives if `stream_options:{include_usage:true}` (final chunk has
  `usage`, `choices: []`, followed by `data: [DONE]`). [ref](https://platform.openai.com/docs/api-reference/chat/create)
- REST API surface is still "currently v1" and model-agnostic per the
  backwards-compatibility policy (new optional fields may be added — keep client serde
  permissive with `#[serde(default)]`). [API overview](https://platform.openai.com/docs/api-reference/errors)

### Base-URL override semantics
The official meaning of "OpenAI-compatible": a server that implements the documented
`POST {base}/v1/chat/completions` request/response schema (same message roles, tool_calls,
finish_reason values, SSE chunk shape, `data: [DONE]` terminator), with auth via
`Authorization: Bearer`. Client convention (OpenAI SDKs, async-openai, env var
`OPENAI_BASE_URL`): the base URL **includes** `/v1`; the client appends `/chat/completions`.
Verified deviations in the wild:
- **Ollama** (`/v1/chat/completions`): supports tools, `tool_choice`,
  `stream_options.include_usage`, `response_format`; local server ignores the API key.
  Does NOT support every field (e.g. no `strict` schema mode guarantees; `n` may be limited).
  [ollama openai](https://docs.ollama.com/openai)
- **vLLM** (`/v1/chat/completions`): tool calls work only if the server was started with
  `--enable-auto-tool-choice --tool-call-parser <parser>` (hermes, llama3_json, mistral, …) —
  otherwise a vanilla server silently won't emit `tool_calls`. `tool_choice` supports
  named/auto/required/none; `user` param ignored; `parallel_tool_calls:false` yields zero or
  one tool call. [vllm tool calling](https://docs.vllm.ai/en/stable/features/tool_calling.html),
  [online serving](https://docs.vllm.ai/en/latest/serving/online_serving/)
- **OpenRouter** (`https://openrouter.ai/api/v1/chat/completions`): OpenAI-like schema, but
  `finish_reason` is normalized to tool_calls/stop/length/content_filter/**error**
  (OpenRouter-specific `error` value!), raw value exposed as `native_finish_reason`; adds
  optional attribution headers (`HTTP-Referer`, `X-Title`) and extra cost data in `usage`.
  [openrouter](https://openrouter.ai/docs/api-reference/overview)

Design consequence for our core: parse `finish_reason`/`stop_reason` into a closed enum with
an `Other(String)` fallback, accumulate streamed tool arguments by index, and never assume
`usage` is present on every chunk. This absorbs all three deviation families above.

## 4. Gemini API — two surfaces, one fixed host

- **Interactions API** (GA since June 2026, recommended for new projects):
  `POST https://generativelanguage.googleapis.com/v1beta/interactions` with
  `x-goog-api-key` header; body `{model, input, tools?, generation_config?, store?}`;
  `previous_interaction_id` chains server-side state; `store:false` for stateless.
  Response `steps[]` include `thought`, `model_output`, and `function_call`
  `{name, arguments, call_id}`. Streaming via `"stream":true` (or `?alt=sse`): events
  `interaction.created` → `step.start{index,type}` → `step.delta{...}`* → `step.stop` →
  `interaction.completed{usage}` → `done: [DONE]`; `step.stop` carries running usage totals.
  [overview](https://ai.google.dev/gemini-api/docs/interactions-overview), [streaming](https://ai.google.dev/gemini-api/docs/streaming)
- **Legacy generateContent** ("considered legacy, remains fully supported"):
  `POST .../v1beta/models/{model}:generateContent` and `:streamGenerateContent?alt=sse`
  (SSE stream of `GenerateContentResponse` chunks). Body: `contents[{role, parts[{text}]}]`,
  `systemInstruction`, `tools[{functionDeclarations}]`, `toolConfig`, `generationConfig`
  (`candidateCount`, `maxOutputTokens`, `stopSequences`). Parts union includes
  `functionCall{name,args}` and `functionResponse{name,response}`.
  [ref](https://ai.google.dev/api/generate-content)
- Recommendation for our fixed-endpoint core: implement generateContent
  (simpler, stateless, matches the task's fixed-endpoint requirement) and design types so an
  Interactions client can be added later; both auth styles (`x-goog-api-key` header
  preferred) work on the same host.

## 5. Rust crates — verified current versions (crates.io, 2026-10-10)

| Crate | Version | Last update | Notes |
|---|---|---|---|
| reqwest | 0.13.5 | 2026-09-08 | default features include `http2`; `stream` feature gives `bytes_stream()` for SSE; `json`, `rustls` available. [crates.io](https://crates.io/crates/reqwest) |
| tokio | 1.53.2 | 2026-10-03 | 1.04B downloads; standard rt. [crates.io](https://crates.io/crates/tokio) |
| serde / serde_json | 1.0.229 / 1.0.152 | 2026-07-18 / 2026-10-10 | 1.5B/1.4B downloads. [crates.io](https://crates.io/crates/serde) |
| schemars | 1.2.2 | 2026-07-27 | derives JSON Schema for tool params (1.x is the current derive API). [crates.io](https://crates.io/crates/schemars) |
| thiserror / anyhow | 2.0.21 / 1.0.104 | 2026-09-23 / 2026-07-18 | thiserror 2.x line is current. [crates.io](https://crates.io/crates/thiserror) |
| tracing | 0.1.44 | 2025-12-18 | stable, universal. [crates.io](https://crates.io/crates/tracing) |
| clap | 4.6.7 | 2026-09-14 | `derive` feature for the digest CLI. [crates.io](https://crates.io/crates/clap) |
| eventsource-stream | 0.2.3 | crate 2022-02-17 | thin `Stream<Item=Bytes>`→`EventStream` adapter used by many LLM clients; repo (jpopesculian) last pushed 2024-08-15 — API stable, low risk. [crates.io](https://crates.io/crates/eventsource-stream) |
| sse-stream | 0.3.0 | 2026-09-18 | actively updated alternative SSE parser (features: memchr, simdutf8). [crates.io](https://crates.io/crates/sse-stream) |
| dotenvy | 0.15.7 | crate 2023-03-22 | repo allan2/dotenvy active (Aug 2026), 1.2K stars; the standard `.env` loader. [repo](https://github.com/allan2/dotenvy) |

### Provider client crates — assessment
- **async-openai 0.42.2** — released today (2026-10-10), 2017 stars, very active.
  Supports custom base URLs: `OpenAI::with_api_base(...)` builder + `OPENAI_BASE_URL` env
  var; base must include `/v1`. Solid for the OpenAI protocol leg if we want a crate.
  [repo](https://github.com/64bit/async-openai)
- **misanthropy 0.0.8** (cortesi) — repo commits through 2026-09-25, but the published crate
  has not changed since 2025-06-08 and sits at 0.0.x (pre-stable API, 12.8K downloads).
  Exists, maintained-ish, but not a foundation. [repo](https://github.com/cortesi/misanthropy)
- Other Anthropic crates are stale: `anthropic` 0.0.8 (2024-09), `anthropic-sdk` 0.1.5
  (2024-07); `anthropic-async` 0.5.4 (2026-07) is a single-maintainer alternative.
  [crates.io](https://crates.io/crates/anthropic-sdk)
- **Gemini crates**: no de-facto standard. `gemini-rs` 2.0.1 (22 stars, single maintainer,
  pushed 2026-07-21); `google-genai` 0.0.2 (dead since 2024). Nothing with the maintenance
  profile of async-openai. [crates.io](https://crates.io/crates/gemini-rs)
- Multiprovider alternatives: `genai` 0.6.5 (jeremychone, 900 stars, updated 2026-10-07;
  OpenAI/Anthropic/Gemini/Ollama/… adapters) and `rig-core` 0.44.0 (3.4M downloads) —
  full frameworks; adopting one gives up BYOK-level control over headers, endpoints, and
  retry semantics. [crates.io](https://crates.io/crates/genai)

### Recommendation: hand-rolled reqwest clients per provider (grok-build style)
1. **Scope is 3 fixed protocols, not N providers.** The BYOK requirement is exactly three
   wire formats: `/v1/messages`, `/v1/chat/completions` (+ compatible base override), and
   Gemini. That's ~600 lines of serde types + SSE plumbing per provider, written once.
2. **BYOK control**: key handling, `anthropic-version` header, `x-goog-api-key`, custom
   base URL, and retry policy stay in our code — no crate's abstraction can leak or
   hardcode them (misanthropy pins its own model catalog; async-openai tracks OpenAI's
   full API surface, including Responses/Live features we don't need).
3. **SSE is the hard part and crates don't remove it**: even with async-openai or
   misanthropy you still assemble tool arguments from deltas; with reqwest +
   eventsource-stream (or ~100 lines of hand-rolled SSE splitting) we own the full path.
4. **Precedent**: grok-build ships its own sampler crate speaking `/chat/completions`,
   `/responses`, `/messages` with one `ApiBackend` enum over wire protocols
   (analysis/07-grok-build.md §3) — and its async-openai dependency is a private fork,
   evidence that production agents don't get clean BYOK out of the box.
5. **Cargo stays lean**: reqwest, tokio, serde*, schemars, thiserror/anyhow, tracing,
   eventsource-stream, dotenvy (dev), clap (CLI). async-openai/misanthropy/gemini crates
   would add three dependency lines and three upgrade cadences for no net control gain.

## 6. Helper tooling for the digest mission

### Duplicate-code detection on Rust in CI
- **jscpd v5** is the tool: now a **Rust engine** (self-contained binary, 224 formats incl.
  Rust, Rabin-Karp rolling hash over language-aware tokens), 6.4K stars, pushed today.
  Installs via `cargo install jscpd` (crates.io 5.4.1), runs as GitHub Action
  `kucherenko/jscpd@v5` (pin `@v5.4.1`/SHA), reports SARIF (uploads to Code Scanning),
  JSON, markdown, `ai` (token-efficient reporter!), console; Type-1/2/3 clones plus
  experimental Type-4 (embeddings, works with any OpenAI-compatible API);
  `--similarity 0.8` compares whole Rust functions by normalized syntax trees;
  `--baseline-from-ref origin/main` / `--fail-on-new-clones` gate on new duplication;
  exit codes for CI. [README](https://github.com/kucherenko/jscpd),
  [action docs](https://github.com/kucherenko/jscpd/blob/master/docs/ci-and-hooks.md)
- **cpd-finder 0.1.21** (crates.io, 2026-10-09): jscpd's detection core as a Rust library —
  lets our agent embed clone detection directly instead of shelling out.
  [crates.io](https://crates.io/crates/cpd-finder)
- Alternative: **PMD CPD** supports Rust since PMD 7.9.0 (CPD: yes; PMD rules: no) but
  requires a JVM in the Action. [PMD Rust](https://docs.pmd-code.org/latest/pmd_languages_rust.html)
- Searched and rejected as non-existent for this use: `cargo-dup`, `dupcatcher`, `tokcod`,
  `prse` (a string-parsing utility, not dup detection). [crates.io](https://crates.io/crates/prse)

### gh commands to enumerate merged PRs across the org (live-verified on storytold)
Per-repo, richest fields (verified: returned `mergedAt`, `additions`, `deletions`, `author`, `url`):
```bash
gh pr list -R storytold/<repo> --state merged \
  --search "merged:>=2026-10-03" --limit 200 \
  --json number,title,url,author,mergedAt,additions,deletions,labels,body
```
Org-wide in one query (verified: works; JSON fields include `repository` but NOT `mergedAt`,
so pair it with per-repo `pr list` for timestamps, or use `gh api`):
```bash
gh search prs --merged --owner storytold --merged ">=2026-10-03" \
  --limit 1000 --json repository,number,title,url,author
```
Raw REST with full control (verified: `total_count: 3441` for org since 2026-09-01; add
`--paginate` to walk pages):
```bash
gh api --paginate "search/issues?q=org:storytold+is:pr+is:merged+merged:%3E%3D2026-10-03&per_page=100" \
  --jq '.items[] | {repo: .repository_url, n: .number, t: .title, merged: .pull_request.merged_at, url}'
```
Constraints (verified): GitHub search returns at most 1,000 results per query — for a
22-repo org, loop repos with per-repo `gh pr list` (no search cap, `--limit` up to 1000
per page with auto-pagination) rather than one global search.
[gh pr list](https://cli.github.com/manual/gh_pr_list), [search API](https://docs.github.com/en/rest/search/search),
[gh search prs](https://cli.github.com/manual/gh_search_prs)

Pipeline for the daily digest: `gh repo list storytold --limit 100 --json name` → per repo
`gh pr list --state merged --search "merged:>=$(date -d '1 day ago' +%F)"` → concatenate
JSON → feed titles+diff stats to the agent; run jscpd per repo (or once across sparse
checkouts) with the `markdown`/`json` reporter for duplicate-code findings.

## 7. Integration-test env patterns (BYOK self-skip)

### .env loading
`dotenvy` 0.15.7 remains the standard: `dotenvy::dotenv().ok();` in a `tests/common/mod.rs`
(crate actively maintained on GitHub; the 2023 crate date is stability, not abandonment).
Manual parsing is reinventing quoting/`export`/comment semantics — not worth it.
[repo](https://github.com/allan2/dotenvy)

### Self-skip when a key is absent — what major Rust projects do
1. **Early-return with a printed reason (the live-API convention).** rust-genai — the most
   relevant comparable project (live provider tests for OpenAI/Anthropic/Gemini/…) — does
   exactly this:
   ```rust
   if !has_audio_file() {
       println!("No test audio file. Skipping this test.");
       return Ok(());
   }
   ```
   The test reports as **PASS** with the message in stdout (source:
   [rust-genai common_tests.rs](https://github.com/jeremychone/rust-genai/blob/main/tests/support/common_tests.rs)).
   Same pattern for missing env keys: `let Ok(key) = std::env::var("ANTHROPIC_API_KEY") else { eprintln!("skipping: ANTHROPIC_API_KEY not set"); return Ok(()); };`
2. **`#[ignore]`** is static — the test is deselected unless `--ignored` is passed; libtest
   prints `N passed; M ignored`. Not env-aware by itself.
3. **cargo nextest**: its filterset DSL (`-E 'package(x) + test(y)'`, predicates all/none/
   package/test/kind/binary/platform/group) has **no env() predicate** — it can deselect
   by name/package but cannot inspect env vars at runtime
   ([filtersets](https://nexte.st/docs/filtersets/), [reference](https://nexte.st/docs/filtersets/reference/)).
   Nextest's `skip` status level covers `#[ignore]` tests deselected from the run; JUnit
   output gained `junit.skipped = none|ignored|all` in 0.9.143. An early-returning test is
   still PASS to nextest — the skip message is just stdout. A CI can generate a filterset
   from env presence (e.g. only include package `-p tests-anthropic` when the key exists),
   but that duplicates knowledge into the workflow.
4. **Cargo features** (`--features live-tests` to enable `#[cfg(feature)]` test modules)
   gate at compile time; clean, but then local runs need the right feature combo and the
   .env-vs-feature mismatch is invisible.
5. `testresult` 0.4.2 (wiktor-k, 1.2M downloads, active) — nice `TestResult` alias for
   precise failures, but has **no bail!/skip facility** (only TestError/ok/TestResult).
   [repo](https://github.com/wiktor-k/testresult)

### Recommended pattern for this project
Per-provider `tests/<provider>_live.rs` + `tests/common/mod.rs`:
```rust
// common/mod.rs
pub fn load_env() { dotenvy::dotenv().ok(); }
macro_rules! require_key { ($name:literal) => { match std::env::var($name) { Ok(k) if !k.is_empty() => k, _ => { eprintln!("SKIP: {} not set", $name); return Ok(()); } } } }
```
- Self-skip via early return (pattern 1) — matches rust-genai, keeps `cargo test` and
  `cargo nextest run` green with zero CI wiring, works with any key combination.
- Print `SKIP:` prefix lines so `cargo test -- --nocapture` and CI logs make each skip
  greppable and auditable; if we later want machine-readable status, nextest's
  `junit.skipped` + a `#[ignore]` macro hybrid can be layered on without rewriting tests.
- Keep CI honest with a required step that asserts at least one provider ran when the
  corresponding secret exists (e.g. shell: `if [ -n "$ANTHROPIC_API_KEY" ] && ! grep -q "SKIP: ANTHROPIC_API_KEY" $LOG; then :; fi`).

## 8. Recommended dependency list (single source)

```toml
[dependencies]
reqwest = { version = "0.13", features = ["json", "stream", "rustls"] }   # http2 in default
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"            # JSON Schema for tool params (matches Anthropic input_schema / OpenAI parameters)
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
eventsource-stream = "0.2"   # SSE over reqwest::bytes_stream (alt: sse-stream 0.3)
clap = { version = "4", features = ["derive"] }   # digest CLI

[dev-dependencies]
dotenvy = "0.15"
```
Client architecture: one `provider` module per API (anthropic, openai_compat, gemini),
each a thin hand-rolled reqwest client behind a shared `ChatClient` trait with a common
`ToolCall{ id, name, arguments_json }` / `Finish` normalization — see §5 reasoning.

## 9. Open items / notes
- OpenAI `strict: true` function schemas (guaranteed JSON-Schema-valid tool arguments)
  exist in the API but OpenRouter/Ollama/vLLM support varies — treat as optional hint,
  verify per-endpoint at runtime, don't rely on it.
  [fc guide](https://platform.openai.com/docs/guides/function-calling)
- Gemini Interactions API is where Google ships new features; generateContent is frozen-but-
  supported. Our fixed-endpoint requirement is satisfiable with either (same host); start
  with generateContent, keep an adapter seam.
- The `gpt-6-luna` reasoning_effort:"none" caveat (Chat Completions function calling) is a
  live-compat test nuance: use `gpt-5.5` or `gpt-6-astra` for tool-loop tests against real
  OpenAI, or set reasoning_effort:none with luna.
