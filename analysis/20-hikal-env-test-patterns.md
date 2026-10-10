# 20 — Hikal .env key names + integration-test patterns (input to the storytold agent core)

Date: 2026-10-10 (slug rs6-hikal-env-tests). Verified read-only against
`/home/ahmed/Projects/hikal_ai/harhaial-desktop`, `.../harhaial-backend`
(root + `.worktrees/bf1`), and `.../docs/analysis`. The live `.env` files were
NEVER opened — every claim below cites `.env.example`, justfile, CI, source,
tests, or docs.

## 1. Env-key inventory — the names the new project must reuse

Canonical committed example: `harhaial-desktop/.env.example:1-8` (the
gitignored `.env` mirrors it; `ui/e2e/native/specs/ssh-remote.native.e2e.ts:7`
states the `.env` carries ONLY `TEST_BYOK_*` keys):

| Key | Example value | Meaning |
|---|---|---|
| `TEST_BYOK_BASE_URL` | `http://localhost:17844/v1` | endpoint base of the OpenAI-compatible BYOK seam |
| `TEST_BYOK_API_KEY` | (empty) | bearer credential — never printed/logged (AD-009) |
| `TEST_BYOK_MODEL` | `harhaial` | chat model id the live tests route to |
| `TEST_BYOK_VISION_MODEL` | `images` | optional vision model id on the SAME endpoint + key |

- Header law (`.env.example:1-2`): "Live-integration keys for the BYOK seam
  (gitignored; never commit real values). … Missing/empty keys make
  `just integration-byok` SKIP, never fail."
- There are NO provider-specific TEST_ keys anywhere in the repo (no
  `TEST_OPENAI_*` / `TEST_ANTHROPIC_*` / `TEST_GEMINI_*`): the desktop seam
  is ONE OpenAI-compatible endpoint — a single triple + an optional vision
  model on the same endpoint.
- Two unrelated test knobs exist; neither is a provider credential:
  `HARHAIAL_TEST_VERSION` (version override, `crates/harhaial-version/src/lib.rs:9,40`)
  and `HARHAIAL_SAMPLER_TEST_TENANT_TOKEN` (sampler fixture token,
  `crates/harhaial-sampler/tests/request_query_and_headers.rs:16`).

Backend repo (server-side proxy keys — a DIFFERENT name set from the BYOK
trio): `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`
(`harhaial-backend/.env.example:78-84`), consumed fail-fast in production by
`required_in_production` at `app/src/config/credentials.rs:32-34` (identical
in `.worktrees/bf1`). That `.env.example` mandates server-side secret storage
only — never in any client, log, or committed file.

**Planning tension to resolve before spec:** the desktop's four names describe
ONE OpenAI-compatible endpoint; the storytold core fixes THREE providers
(Anthropic — fixed endpoint; OpenAI — overridable base URL; Gemini — fixed
endpoint). Reusing the same names verbatim leaves no slot for three distinct
provider keys. The exact strings above are the owner's constraint; HOW three
providers map onto them (e.g. the OpenAI lane owns the `TEST_BYOK_*` triple,
Anthropic/Gemini rows keyed by an owner-approved extension) is an open spec
decision the planner must surface.

## 2. Who consumes each name — recipes, corpora, CI

Every live corpus is `#[ignore]`d and runs ONLY via its `just integration-*`
recipe — `cargo test -p <crate> --test <name> -- --ignored --nocapture`
(justfile:38-123); none is part of `just check`, so the deterministic gate
never depends on credentials. CI mirrors every recipe verbatim
(`.github/workflows/ci.yml:82-145`, the "GH runners lack just" lockstep law);
runners carry no `.env`, so every row prints its SKIP reason and exits 0.

| Recipe | Corpus (tests/) | Keys read |
|---|---|---|
| `integration-byok` (justfile:44-45) | `byok-provider/tests/integration.rs` | the triple |
| `integration-plan-gate` (justfile:53-54) | `harhaial-plan-gate/tests/integration_live.rs` | the triple |
| `integration-agent` (justfile:62-63) | `byok-provider/tests/integration_tools.rs` + `integration_auth.rs` | the triple |
| `integration-stream-tools` (justfile:71-72) | `byok-provider/tests/integration_stream_tools.rs` | the triple |
| `integration-dialect-mount` (justfile:79-80) | `byok-provider/tests/integration_dialect_mount.rs` | the triple |
| `integration-cron-fire` (justfile:87-88) | `byok-provider/tests/integration_cron_fire.rs` | the triple |
| `integration-vision` (justfile:95-96) | `byok-provider/tests/integration_vision.rs` | triple; vision model replaces the model slot |
| `integration-engine-loop` (justfile:103-104) | `byok-provider/tests/integration_engine_loop.rs` | the triple |
| `integration-scenarios` (justfile:113-114) | `byok-provider/tests/integration_scenarios.rs` | triple; vision model gates vision rows only (:756) |
| `integration-multiturn` (justfile:122-123) | `byok-provider/tests/integration_multiturn.rs` | the triple |

- Present in the tree but wired to NO recipe/CI step (027 in-flight work, as
  of today): `crates/byok-provider/tests/integration_multimodal.rs` (reads
  the quadruple) and `src-tauri/tests/runs-live-history-integration.rs`
  (reads the triple; `#[ignore = "live endpoint; requires .env TEST_BYOK_*
  triple"]`).
- Native e2e (WebdriverIO over tauri-driver) does NOT read the names
  directly: the runner sources the repo-root `.env` in bash
  (`set -a; source ../.env; set +a`) and REMAPS them —
  `E2E_PROVIDER_BASE_URL="$TEST_BYOK_BASE_URL"`,
  `E2E_PROVIDER_MODEL="$TEST_BYOK_MODEL"`,
  `E2E_PROVIDER_KEY="$TEST_BYOK_API_KEY"`
  (`.agents/skills/native-e2e/SKILL.md:56-57`; specs read the E2E_ names at
  `ui/e2e/native/specs/agent-core.native.e2e.ts:31-34`). Log hygiene: pipe
  runner output through `grep -vE "TEST_BYOK|API_KEY|sk-"`
  (SKILL.md:24; also `docs/app/native-e2e.md:57`).

## 3. The skip idiom to adopt (exact Rust pattern)

`crates/byok-provider/tests/integration.rs:13-77` is the reference — copy
this shape verbatim into the storytold integration tests:

1. **Manual .env loader** (NO dotenvy in tests) — walks up from
   `env!("CARGO_MANIFEST_DIR")` at most 3 levels looking for `.env`, parses
   simple `KEY=VALUE` lines (skips blank/`#` lines, no quoting/exports),
   then merges the real process env ON TOP (process env wins).
   integration.rs:24-51.
2. **Resolver returning Option** — `fn test_keys() -> Option<TestKeys>` reads
   the three names and returns `None` when ANY is missing OR EMPTY
   (integration.rs:53-66); `TestKeys { base_url, api_key, model_id }`
   (:15-19).
3. **Guard macro per test** —

   ```rust
   macro_rules! require_keys {
       ($keys:ident) => {
           let Some($keys) = test_keys() else {
               eprintln!("SKIP: TEST_BYOK_BASE_URL / TEST_BYOK_API_KEY / TEST_BYOK_MODEL are not set (no .env) — the live corpus is off");
               return;
           };
       };
   }
   ```

   (integration.rs:70-77) — the `eprintln!` + early `return` makes an
   unconfigured run PASS (exit 0), never fail.
4. **`#[ignore]` on every live test** — `#[ignore = "live endpoint; run via
   `just integration-byok`"]` (integration.rs:80); most corpora use
   `#[ignore = "live corpus: requires TEST_BYOK_* in .env"]`
   (e.g. integration_multiturn.rs:110). `cargo test` skips them by default;
   the recipe's `--ignored` opts in.
5. **Message format law** — `SKIP: <KEY_A> / <KEY_B> / <KEY_C> are not set
   (no .env) — the live <corpus> corpus is off`, the corpus name varying
   ("multi-turn", "vision", "dialect-mount", "engine-loop" …). Variant:
   an optional key inside a LIVE corpus degrades to a row-level skip —
   `SKIP: TEST_BYOK_VISION_MODEL is not set — the vision rows are off`
   (integration_scenarios.rs:756).
6. **Key-value hygiene (AD-009)** — key NAMES may be printed, values NEVER;
   live assertions are STRUCTURAL (ids, shapes, event order), never
   token-exact (`specs/027-agentic-chat-core/tasks.md:228`).

## 4. How each repo loads .env in dev

- Desktop TEST harness: the manual loader of §3 — no dotenvy anywhere in
  `crates/`/`src-tauri` test code. Key names appear ONLY in test files,
  justfile comments, CI comments, and docs.
- Backend: `dotenvy::dotenv().ok()` at the TOP of the thin bin
  (`app/src/main.rs:15`; `dotenvy = "0.15.7"` workspace dep,
  `Cargo.toml:36`) — the LIB never loads `.env` itself; `Config::from_env`
  reads process env fail-fast. Tests receive env from the process
  (CI/gate), never from a file. Same in `.worktrees/bf1`.
- Recommendation for storytold: the desktop pattern (test-local manual
  loader, process env wins) fits a library-shaped core — zero extra
  dependency, walk-up discovery keeps `cargo test -p <crate>` working from
  any cwd, and the merge order makes CI overrides trivial.

## 5. The vendored Rust provider stack (what the owner already has)

- `crates/providers/src/lib.rs:11` is an EMPTY placeholder (an adapters
  module with no code). The real stack: `harhaial-sampler` (HTTP client +
  L2 stream transforms + L3 actor; rebranded fork of grok-build's
  `xai-grok-sampler`, Apache-2.0 — `crates/harhaial-sampler/provenance.md:1-15`)
  + `harhaial-sampling-types` (wire types) + `byok-provider` (the
  app-facing BYOK wrapper) + `provider-catalog` (template catalog).
- AD-005 (`docs/06-tech-decisions.md:53-61`): one Provider trait + registry,
  five adapter kinds in v1 — `openai`, `anthropic`, `google`, `ollama`,
  `openai_compatible`. Partially superseded by AD-010 (backend-gated
  access); realized as plugin registrations (AD-012) over the vendored
  stack (AD-011).
- **Dialect is DATA, not adapter kind**: `ApiBackend` enum —
  `ChatCompletions` (`/chat/completions`), `Responses` (`/responses`),
  `Messages` (Anthropic `/v1/messages`), serde snake_case on the wire
  (`crates/harhaial-sampling-types/src/types.rs:1043-1051`). A catalog
  entry carries `{ backend, base_url }` (`crates/provider-catalog/src/types.rs:57-65`).
- **Auth is a SEPARATE knob from dialect**: `AuthScheme::{Bearer
  (default), XApiKey}` (`crates/harhaial-sampler/src/config.rs:19-23`).
  `Bearer` → `Authorization: Bearer <key>`; `XApiKey` → `x-api-key: <key>`
  (applied at client build, `client.rs:505-532`). The key VALUE is never
  logged — only header name + non-emptiness (client.rs:498-511).
- **SamplerConfig** (config.rs:44-120) — per-request knobs: `api_key`,
  `base_url`, `model`, `api_backend`, `auth_scheme`, `extra_headers`
  (IndexMap applied verbatim — the single injection point for e.g. an
  `anthropic-version` header), `query_params`, `env_http_headers`
  (header-name → env-var, resolved at build, never persisted),
  `max_retries`, `bearer_resolver` (fresh bearer per request),
  `attribution_callback` (401 hook), `stream_tool_calls` (ON by default).
- **Builtin catalog** (`crates/provider-catalog/catalog/builtin.json`, 7
  templates — endpoint/dialect as DATA):
  anthropic = `messages` @ `https://api.anthropic.com/v1`;
  openai = `responses` @ `https://api.openai.com/v1`;
  google = `chat_completions` @
  `https://generativelanguage.googleapis.com/v1beta/openai`
  (Gemini consumed via its OpenAI-COMPATIBLE endpoint, Bearer key);
  deepseek = `messages` @ `https://api.deepseek.com/anthropic`
  (Anthropic-compat); openrouter/groq/ollama = `chat_completions`
  (the OpenAI-compat catch-all).
- **Streaming**: reqwest + `eventsource_stream::Eventsource` SSE parsing,
  `Accept: text/event-stream` (client.rs:16,1037). Streamed tool-call
  fragments (`choices[0].delta.tool_calls`) fold by wire `index` into
  complete calls — id/name latched from the FIRST fragment, arguments
  concatenated in arrival order (`byok-provider/src/provider.rs:203-234`,
  the FR-024 fold `assemble_tool_calls`).
- **Dispatch is dialect-aware at the ACTOR layer**:
  `actor/request_task.rs:538-615` matches `client.api_backend()` and calls
  `conversation_stream` / `conversation_stream_responses` /
  `conversation_stream_messages`; the Messages family POSTs
  `{base_url}/messages` (`create_message_stream`, client.rs:1662-1700).
  The Anthropic Messages request body
  (`harhaial-sampling-types/src/messages.rs:12-33`): `max_tokens`
  REQUIRED, `system` a TOP-LEVEL param, tools/tool_choice/stop_sequences/
  thinking optional.
- **The honest gap** (`docs/app/provider-catalog.md:82-87`): the BYOK-FACING
  streaming paths always POST `{base_url}/chat/completions` — the byok
  seam (`byok-provider/src/provider.rs:239-263` + `config.rs:52-69`) calls
  `client.chat_completion_stream` directly, which hardcodes the
  `chat/completions` path AND defaults to Bearer auth, so a `Messages`-
  dialect mount is not actually reachable through BYOK today; the live
  dialect rows assert exactly that.
- **No `anthropic-version` header anywhere** in the vendored stack (grep:
  zero hits) — the upstream target was a tolerant proxy; direct
  api.anthropic.com calls NEED it. For storytold's fixed Anthropic
  endpoint: add it (a default `extra_headers` entry or a first-class
  config knob).
- **Engine ports** (the agent-core seam,
  `crates/engine/src/run/context.rs:47-94`):
  `trait ProviderTransport { fn send_stream(&self, &ModelRequest) ->
  Result<Box<dyn ChunkTransport>, TransportFailure> }` — `ModelRequest`
  carries ONLY `{model, prompt}`; auth lives in the transport adapter,
  never in the engine. `trait ToolGate { fn pre_execute(call_id, name,
  arguments) -> ToolGateOutcome }` with fail-closed
  `Allow | Deny{reason}` — the pre-dispatch veto every tool call passes
  through.

## 6. What to mirror vs differ in the storytold agent core

MIRROR (proven, reuse as-is):
- The §3 skip idiom verbatim (Option resolver + require_keys! macro +
  `#[ignore]` + recipe `--ignored --nocapture` + CI mirror).
- Dialect-as-data: one enum (chat_completions / messages / gemini) selected
  per provider config, dispatched at ONE seam — the actor pattern
  (request_task.rs:538), NOT the byok-provider shortcut.
- The FR-024 streamed-tool-call fold (index-grouped fragment assembly).
- Structural assertions in live tests; key names printable, values never.
- `test_connection` health probe: GET `{base_url}/models` + bearer, sized
  Ok/Fail outcome, never a panic (`byok-provider/src/provider.rs:60-117`).

DIFFER (the desktop's gap is our requirement):
- Anthropic: native Messages API at a FIXED endpoint — `x-api-key` auth
  (AuthScheme::XApiKey exists but is unwired for BYOK), `anthropic-version`
  header (absent in the vendored stack), `max_tokens` required, top-level
  `system`.
- OpenAI: Chat Completions with an OVERRIDABLE base URL — the lane closest
  to what the desktop BYOK seam does today.
- Gemini: the desktop's pragmatic route is Gemini's OpenAI-compatible
  endpoint (`.../v1beta/openai`, Bearer) riding chat_completions; a native
  `generateContent` adapter differs (the backend parses its SSE
  `usageMetadata` at `app/src/proxy/providers/google.rs:8-35` — the native
  wire was studied, not adopted client-side).

## 7. Backend skip/refusal idioms (the sibling pattern)

- Gate stage 7 (`scripts/quality-gate.sh:144-152`): `TEST_DATABASE_URL`
  unset → loud yellow `⚠ SKIP (no scratch DB configured: set
  TEST_DATABASE_URL)` and exit 0; SET-but-unusable → RED exit 1 —
  "a silently-skipped stage would be a weakened gate; a claimed scratch
  DB that cannot be migrated is a failure, never a pass" (:139-143). The
  loud-skip-vs-fail split is the rule to carry into storytold's GitHub
  Action: absence skips; presence that cannot run FAILS.
- DB-context TESTS refuse rather than skip:
  `app/tests/common/db/guard.rs:56-66` returns a typed `HarnessRefusal`
  ("There is NO fallback") — an unconfigured DB is a contract violation.
  Keep the graceful skip ONLY for live-provider rows.
- `.worktrees/bf1` is the same code plus a 12-stage gate (adds an OpenAPI
  contract structural check at stage 9); skip idioms identical
  (`.worktrees/bf1/scripts/quality-gate.sh:144`,
  `.worktrees/bf1/AGENTS.md:39-74`).

## 8. Parent docs/analysis — agent-core-relevant insights

`/home/ahmed/Projects/hikal_ai/docs/analysis/` holds the harness analyses
that shaped Harhaial (plus security corpora: anti-reverse-engineering,
runtime-memory-tampering-defense, wire-protocol-encryption-and-obscuration,
unforeseen-operational-and-security-gaps, speckit-coverage, and the
ENFORCEMENT-REGISTER that dispositioned them). The three harness analyses:

- **deepseek-harness-analysis.md** (dsh): the "everything is a plugin"
  Cordis model — services with inject/provide + reversible disposers,
  waterfall-interceptable pipelines (agent/pre-step, tools/pre-execute),
  append-only session log as the durable source of truth with model
  history as a projection, provider-neutral StreamChunk vocabulary + one
  BlockAssembler (§2-6). For storytold: the tool-loop seams (pre-execute
  veto, typed stream chunks) and the log-as-truth stance (:79-92).
- **kimi-code-analysis.md**: the kosong provider layer is the closest
  requirement match — `ChatProvider.generate(...) → StreamedMessage`
  async-iterable + post-drain usage/finishReason, PER-REQUEST auth
  resolution (providers hold no mutable credential state), a normalized
  FinishReason taxonomy, capability-declaring model catalog, zod→
  JSON-schema tool schemas (§3.4 :84-95, §4.4); plus tool-runtime engine
  services (args validation, path ACL, bounded output truncation) every
  tool inherits (§3.5, §4.6).
- **grok-build-analysis.md**: the engine-side reference — replayable run
  journal (typed host-call entries + req-hash divergence detection),
  PauseKind/WorkflowOutcome closed vocabulary (Verification a first-class
  pause reason), typed tool runtime ("arbitrarily many Progress ending in
  exactly one Terminal"; approval a 3-value policy), and the provider
  layer as a typed superset with a CLOSED retry taxonomy (RetryDecision
  enum, not a boolean) + doom-loop detection (§3.1-3.3, §3.7, §4.1,
  §4.11). This is literally the stack harhaial-sampler was forked from
  (provenance.md).

## 9. Quick reference for the storytold spec

- Committed key contract: `TEST_BYOK_BASE_URL`, `TEST_BYOK_API_KEY`,
  `TEST_BYOK_MODEL`, `TEST_BYOK_VISION_MODEL`
  (harhaial-desktop/.env.example:3-8) — the names the owner requires.
- Backend provider-key names (if the Action runtime env wants them):
  `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `GOOGLE_API_KEY`
  (harhaial-backend/.env.example:82-84).
- Skip law: missing/empty key ⇒ eprintln SKIP + pass; set-but-broken ⇒
  fail loudly. Live tests `#[ignore]`d; recipe `--ignored --nocapture`;
  CI mirrors recipes; live corpora never gate the deterministic gate.
- The desktop's BYOK seam is single-endpoint (OpenAI-compat); native
  Anthropic Messages + native Gemini support is the delta storytold must
  build that the desktop deferred (the "honest gap").
