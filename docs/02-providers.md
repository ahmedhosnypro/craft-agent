# 02 — Providers

Three wire adapters — Anthropic Messages, OpenAI Chat Completions,
Gemini generateContent — implement one streaming `Provider` trait over
the neutral vocabulary of `craft-core`; all three are clean-room
implementations of the providers' public documentation. The Gemini
dialect mounts twice — the public API-key host and the Vertex project
endpoint — over the same encoder, decode lane, and error taxonomy.
Wire differences live in the adapters, never in the callers: a
`Backend` enum mounts exactly one dialect and the runtime dispatches
through it without knowing which wire it is talking to.

## Constructor seam

An adapter is constructed with three explicit values — base URL, model
id, API key — and nothing else; the Vertex mount takes project,
location, and model instead, deriving its endpoint from the location.
No adapter reads the environment at mount; the app layer resolves
configuration and mounts the backend, so a lane or corpus can mount a
loopback endpoint without touching process state. The base URL joins
the endpoint path verbatim (a trailing slash is trimmed), so the
request itself never carries endpoint state. The one lazy exception
is the Vertex credential (below): the ADC file resolves on the first
request and stays cached for the process lifetime. The Anthropic
adapter alone carries one post-construction override —
`with_auth_scheme`, how the key rides — mounted from `ANTHROPIC_AUTH`
by the app layer.

## BYOK environment

The app layer (`craft-app` config) reads named variables only — a
missing or empty variable is a hard error that names itself; there is no
ambient fallback:

| Variable | Role |
|---|---|
| `CRAFT_AGENT_PROVIDER` | Required. Selects the dialect: `openai`, `anthropic`, `gemini`, or `vertex`; anything else is a typed rejection. |
| `CRAFT_AGENT_MODEL` | Required. The model id sent to the provider. |
| `OPENAI_API_KEY` | Required for the `openai` dialect. |
| `OPENAI_BASE_URL` | Optional override of the OpenAI base URL, so any OpenAI-compatible lane works; default `https://api.openai.com/v1`. |
| `ANTHROPIC_API_KEY` | Required for the `anthropic` dialect. |
| `ANTHROPIC_BASE_URL` | Optional override of the Anthropic base URL; default `https://api.anthropic.com`. |
| `ANTHROPIC_AUTH` | Optional for the `anthropic` dialect; how the key rides. `x-api-key` (the default, and what unset or empty means) or `bearer`; anything else is a typed rejection. |
| `GOOGLE_API_KEY` | Required for the `gemini` dialect. |
| `GEMINI_BASE_URL` | Optional override of the Gemini base URL; default `https://generativelanguage.googleapis.com` (the adapter appends the version and model path itself, so an override stays host-shaped). |
| `GOOGLE_VERTEX_PROJECT` | Required for the `vertex` dialect. The Google Cloud project id. |
| `GOOGLE_VERTEX_LOCATION` | Required for the `vertex` dialect. The service location; `global` rides the unprefixed host, a regional location its own prefixed host. |
| `GOOGLE_APPLICATION_CREDENTIALS` | The `vertex` dialect's ADC credential file; unset means the documented gcloud default path. Read by the adapter on the first request, never at mount. |
| `TEST_BYOK_GEMINI_API_KEY` | Live-corpus knob for the Gemini corpus; empty means the corpus skips. |
| `TEST_BYOK_GEMINI_MODEL` | Live-corpus knob: the model id the Gemini corpus requests; empty means the corpus skips. |
| `TEST_BYOK_GEMINI_BASE_URL` | Live-corpus knob: optional; empty means the production Gemini host, a set value points the corpus at a loopback. |
| `GH_TOKEN` | Optional; handed to the gh tool. |

The binary reads only the key matching the selected provider and
removes provider-key variables from every child process.

The `TEST_BYOK_GEMINI_*` rows are corpus knobs, not run configuration:
the live Gemini corpus reads them from the repo-root `.env`, and a
missing or empty required knob prints a SKIP line naming the knob —
never its value — and passes.

## Endpoints and authentication

| Dialect | Endpoint | Authentication |
|---|---|---|
| OpenAI Chat Completions | `{base}/chat/completions` | `Authorization: Bearer <key>` |
| Anthropic Messages | `{base}/v1/messages` | `x-api-key: <key>` by default, or `Authorization: Bearer <key>` when `ANTHROPIC_AUTH=bearer` opts in; every request also carries `anthropic-version: 2023-06-01` |
| Gemini generateContent | `{base}/v1beta/models/{model}:streamGenerateContent?alt=sse` | `x-goog-api-key: <key>` |
| Vertex generateContent | `https://{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/publishers/google/models/{model}:streamGenerateContent?alt=sse` | `Authorization: Bearer <ADC access token>` |

Every request carries `Content-Type: application/json`. Default hosts:
`https://api.openai.com/v1` (the version segment is part of the base,
so an override stays fully shaped), `https://api.anthropic.com`, and
`https://generativelanguage.googleapis.com` (the adapter appends the
version and model path itself, so an override stays host-shaped). The
Vertex endpoint derives from the location: `global` rides
`https://aiplatform.googleapis.com`, every regional location its own
`https://{location}-aiplatform.googleapis.com` host. A
non-2xx body is classification input only and is read capped at 64 KiB,
so a hostile server cannot balloon the process. Provider host literals
are adapter data: they are allowed only under `crates/craft-providers/`
(enforced by `scripts/identifier-scan.sh`).

## The two retry ladders

Retries live at exactly one level per failure. The connect ladder,
inside each adapter, replays the HTTP POST before any byte is decoded;
the step ladder, in the runtime, replays a whole model step after its
stream died mid-turn. The seam splits the levels structurally: a
failure surfacing from `stream()` is connect-phase and already climbed
the adapter ladder, so the runtime never replays it again — only a
death inside a connected stream, or an empty-but-complete turn, is the
step ladder's to replay.

### Connect ladder (per request)

One shared ladder (`craft-core` constants), mounted identically by every
adapter:

| Constant | Value | Meaning |
|---|---|---|
| `RETRY_MAX_ATTEMPTS` | 6 | Attempts per request, connection failures only. |
| `RETRY_BACKOFF_INITIAL_SECS` | 2 | First backoff step; doubles each attempt. |
| `RETRY_BACKOFF_CAP_SECS` | 30 | Backoff ceiling; a live `Retry-After` is clamped to it. |
| `RATE_LIMIT_STREAK` | 5 | Consecutive 429s that surface the `RateLimit` error immediately. |

Only `RateLimit`, `Transient`, and `Transport` retry: authentication and
client errors fail fast. A `Retry-After` header (delta-seconds form
only; HTTP-date and junk fall back to the ladder) overrides the
computed backoff, clamped to the cap. The Gemini dialect adds one
structured fallback: with no live header, a `google.rpc.RetryInfo`
`retryDelay` inside the error `details` array (a
`google.protobuf.Duration` string) carries the delay — unparseable or
negative values fall back to the ladder, and the same cap clamps it.

### Step ladder (per model step)

`CRAFT_AGENT_STEP_RETRIES` bounds the total attempts per step: the
default is 3, `0` restores fail-fast (one attempt), and an unparseable
value is a typed error naming the knob. A replay discards the partial
fold and resends the same request from the intact transcript. Backoff
doubles from 500 ms, capped at 32 s with ±25 % jitter; a live
`Retry-After` (delta-seconds form) replaces the backoff, clamped to the
cap.

Replayed: `Transport`, `Transient`, `Timeout`, `IdleTimeout`,
`StreamClosed`, a mid-stream `RateLimit`, and an empty-but-complete
turn (`EmptyResponse`) unless its terminal is `ContentFilter`. Never
replayed: `Auth`, `InvalidRequest`, `ContextWindow`, `PayloadTooLarge`,
`QuotaExhausted`, a filtered empty turn, and every fold failure — a
malformed response is semantic, not transient. Each replay emits a
`turn_retried` stream-json event and charges its usage to the run
budget before the next attempt fires.

## The decode lane

Every adapter shares one lane shape — the buffered-terminal
unfold. SSE bytes pass through a bounded reader (`craft-core`): a
1 MiB per-event cap, a per-chunk idle timeout (default 300 s), a
leading-BOM strip, and typed malformed-event errors. Each lane then
folds its wire events into the neutral stream:

- Text and tool-call fragments surface as they arrive (`ToolCallDelta`
  keyed by the wire index; the Gemini lane assigns the indices itself).
- The terminal stop reason is buffered, never emitted inline; usage
  frames merge into it as they arrive (`Usage::merge` keeps the
  per-bucket maximum, so repeated reports never double-count).
- When the wire finishes, the lane flushes exactly one `TurnFinished`
  carrying the stop reason and the merged usage. A wire that ends
  without a buffered terminal surfaces `StreamClosed`.
- A delta after the terminal is a typed transport error; a malformed
  frame or transport error after the terminal stands as the recorded
  turn, while one before it is a typed failure.

Per dialect:

- **OpenAI** — data-line chunks; only the first choice decodes (the
  request never asks for more than one completion); `[DONE]` ends the
  lane; mid-stream error envelopes map onto the taxonomy.
- **Anthropic** — named events dispatched by documented name; ping and
  unknown names are skipped, so a newer protocol never breaks the lane.
  Blocks register at their wire index: a `tool_use` head emits the
  call's id and name at once, `input_json_delta` fragments the
  arguments. `thinking` and `redacted_thinking` heads open a native
  accumulator: `thinking_delta` and `signature_delta` append under a
  1 MiB per-block cap, and at the stop the lane emits exactly one
  reconstructed native block — `{"type":"thinking", "thinking":…}`
  with the `signature` field riding only when one arrived, or the
  redacted block's data verbatim; an empty thinking block is a legal
  skip, a redacted block without data is a typed failure. A delta on
  the wrong block kind, or without a start, is a typed transport
  error.
- **Gemini** — camelCase chunk JSON; `usageMetadata` merges
  (`thoughtsTokenCount` lands in `reasoning_tokens`); a
  `promptFeedback.blockReason` is a `ContentFilter` terminal. Thought
  text rides a `{"text":…, "thought":true}` native block and a
  signature on any part rides a bare `{"thoughtSignature":…}` one —
  both surface as native-block events and replay verbatim as leading
  model parts; one arriving after the terminal is a typed transport
  error. A `functionCall` part lands whole on the next
  adapter-assigned index.

## Stop reasons

One neutral union across every dialect; unknown finish reasons land in
`Other(raw)`, and the fold caps the raw string at 64 bytes on a char
boundary:

| Neutral | OpenAI | Anthropic | Gemini |
|---|---|---|---|
| `EndTurn` | `stop` | `end_turn` | `STOP` |
| `MaxTokens` | `length` | `max_tokens` | `MAX_TOKENS` |
| `ToolUse` | `tool_calls`, legacy `function_call` | `tool_use` | — |
| `StopSequence` | — | `stop_sequence` | — |
| `ContentFilter` | `content_filter` | — | `SAFETY`, `RECITATION`, `LANGUAGE`, `BLOCKLIST`, `PROHIBITED_CONTENT`, `SPII` |
| `Refusal` | — | `refusal` | — |
| `PausedTurn` | — | `pause_turn` | — |
| `MalformedToolCall` | — | — | `MALFORMED_FUNCTION_CALL`, `UNEXPECTED_TOOL_CALL` |
| `ContextWindowExceeded` | — | `model_context_window_exceeded` | — |

## Error taxonomy

A closed set (`craft-core` `ProviderError`): `Auth`, `RateLimit`
(carrying the live `Retry-After`, or the structured `RetryInfo` delay
on the Gemini dialect), `QuotaExhausted`, `Transient`,
`PayloadTooLarge`, `ContextWindow`, `InvalidRequest`, `Transport`,
`IdleTimeout`, `StreamClosed`, and `EmptyResponse`. HTTP classification
is shared: 401/403 → `Auth`; 429 → `RateLimit`; 413 →
`PayloadTooLarge`; 5xx and 408 → `Transient`; other 4xx →
`InvalidRequest`; anything else → `Transport`. Body-driven refinements
run before the taxonomy closes — a context-window rejection riding a
plain 400 is `ContextWindow`, and a spent daily quota is
`QuotaExhausted`: OpenAI by an `insufficient_quota`-shaped error
(code, type, or message), Gemini by the machine-readable code
`quota_exceeded` or a message naming the exceeded daily quota. The
Gemini marker rule is fail-open — without an explicit marker a 429
`RESOURCE_EXHAUSTED` stays a retryable `RateLimit`. A spent quota is
deterministic and fails fast instead of riding a ladder.

- OpenAI: an error code or message containing `context_length` /
  `context length`.
- Anthropic: a message containing `context window` or
  `prompt is too long`.
- Gemini: a message containing `context window`.

Mid-stream error events (a chunk or named event carrying an error
envelope) map onto the same taxonomy:

| Wire kind | Taxonomy |
|---|---|
| OpenAI `rate_limit` | `RateLimit` |
| OpenAI `insufficient_quota` (code, type, or message) | `QuotaExhausted` |
| OpenAI `server_error`, `service_unavailable` | `Transient` |
| OpenAI `invalid_request_error` | `InvalidRequest` |
| OpenAI `authentication_error`, `permission_error` | `Auth` |
| Anthropic `rate_limit_error` | `RateLimit` |
| Anthropic `overloaded_error`, `api_error`, `timeout_error` | `Transient` |
| Anthropic `invalid_request_error`, `not_found_error` | `InvalidRequest` |
| Anthropic `authentication_error`, `permission_error` | `Auth` |
| Gemini `UNAUTHENTICATED`, `PERMISSION_DENIED` | `Auth` |
| Gemini `RESOURCE_EXHAUSTED` with a daily-quota marker | `QuotaExhausted` |
| Gemini `RESOURCE_EXHAUSTED` otherwise | `RateLimit`, carrying the `RetryInfo` delay when the details array holds one |
| Gemini `INTERNAL`, `UNAVAILABLE`, `DEADLINE_EXCEEDED`, `UNKNOWN` | `Transient` |
| Gemini envelope whose message names a context window | `ContextWindow` |
| Gemini `INVALID_ARGUMENT`, `FAILED_PRECONDITION`, `OUT_OF_RANGE` | `InvalidRequest` |
| anything else | `Transport` |

## Request encoding

- **OpenAI** — the system prompt rides as a leading system message;
  tools ride as `function` entries with `tool_choice: auto`. A
  thinking level rides as `reasoning_effort` verbatim (`off` normalizes
  to `none`), and only when set.
- **Anthropic** — top-level `system`; `max_tokens` is required and
  clamped to the family ceiling (below). Assistant turns are content
  blocks — replayed native blocks first, then `text` and `tool_use`
  (arguments parse into the input object the wire requires); an
  assistant turn with no content at all is a typed rejection.
  Consecutive tool results flush as one user message of `tool_result`
  blocks; the error flag marks rows the tool layer refused. User text
  and tool-result content become block arrays when images ride.
  Sampling rides only when set: `temperature` and `top_p` as finite
  JSON numbers (a non-finite value is a typed rejection),
  `stop_sequences` when non-empty; anything unset keeps the provider
  default. `tool_choice` (`auto`/`any`/`tool`) rides only with tools.
  A thinking level rides as `output_config.effort` verbatim, or as
  `thinking: {type: disabled}` for `none`/`off` — the two wire forms
  are mutually exclusive, so the budget form never rides alongside a
  level. A final pass marks the three prompt-cache breakpoints.
- **Gemini** — `systemInstruction` parts;
  `generationConfig.maxOutputTokens` (clamped to a known model's output
  cap — the registry, below); tools as `functionDeclarations`
  with `parametersJsonSchema`; assistant turns are `model` parts. A
  `functionResponse` requires the function name, resolved from the
  assistant call it answers — a result with no pairing call is a typed
  rejection. An error result rides the documented error key; a
  JSON-object result rides verbatim; any other text wraps in `output`.
  Sampling rides in `generationConfig` only when set: `temperature`
  and `topP` as finite JSON numbers (a non-finite value is a typed
  rejection), `stopSequences` when non-empty; anything unset keeps the
  provider default. `tool_choice` rides as
  `toolConfig.functionCallingConfig` — `auto` → mode `AUTO`, `any` →
  mode `ANY`, a named tool → mode `ANY` plus `allowedFunctionNames` —
  and only with tools. The thinking request rides as
  `generationConfig.thinkingConfig`: `thinkingBudget` always,
  `includeThoughts` only when the caller asked for surfaced thoughts;
  a known non-thinking model omits `thinkingConfig` entirely (below).
  A thinking level overrides that shape: `none`/`off` rides
  `thinkingBudget: 0`, a known level (`low`/`medium`/`high`/`xhigh`/
  `max`) rides uppercased as `thinkingLevel`, and any other string
  rides verbatim as `thinkingLevel`.
  Image attachments ride as `inlineData` parts — beside the text part
  of a user message (the text part rides only when non-empty), after
  the `functionResponse` part of a tool result; a url image has no
  inline form here and is a typed rejection before any wire bytes, and
  a known non-multimodal model rejects image input the same way (below).

## Anthropic Messages specifics

### The max_tokens family ceiling

The Messages API rejects a `max_tokens` above each model family's
output cap, so the encoder clamps the requested value before the body
is built. The family token and the first two numeric tokens of the id
decide the cap; an id naming no family passes through unclamped, and
a bare family id aliases the current generation:

| Family | Generation | Cap |
|---|---|---|
| opus | 3.x | 4096 |
| opus | 4.0–4.4 | 32000 |
| opus | 4.5 | 64000 |
| opus | 4.6 and later | 128000 |
| sonnet | 3.x | 8192 |
| sonnet | 4.0–4.5 | 64000 |
| sonnet | 4.6 and later | 128000 |
| haiku | 3.0 | 4096 |
| haiku | 3.5 | 8192 |
| haiku | 4.0 and later | 64000 |

### Prompt-cache breakpoints

One final pass over the encoded body marks up to three breakpoints
with `cache_control: {"type": "ephemeral"}` — the system prompt (a
string is promoted to one marked text block), the last tool
definition, and the last content block of the last message (promoted
from a plain string when non-empty). Only the content-block types the
API accepts are markable (`text`, `image`, `tool_result`,
`tool_use`), and a block already carrying a breakpoint is never
marked twice; empty or unmarkable blocks carry none.

### Image attachments

Images ride on user and tool_result content as `image` blocks: a
base64 source with a whitelisted media type — `image/png`,
`image/jpeg`, `image/gif`, `image/webp`; anything else is a typed
`InvalidRequest` — or a url source. Message content stays a plain
string when no images ride; with images it becomes a block array, and
the text block is omitted when the text is empty.

### Tool-call id sanitation

Tool-call ids travel in one deterministic wire-safe form: `[A-Za-z0-9_-]`
kept, every other char replaced with `-`, truncated to 64 bytes on a
char boundary. A sanitized id that collides with an earlier one gets a
numeric suffix, re-truncated to the cap. `tool_result` ids resolve
through the same map, so each result answers the id its call sent;
an id never registered in the turn sanitizes directly.

## Gemini generateContent specifics

### The model registry

The encoder consults a registry of documented per-model limits. A
`ModelInfo` row carries the input token limit, the output token
limit, a thinking flag, and an image-input flag; an id resolves by
longest matching prefix, so narrower rows — lite variants, the
text-only TTS variants — outrank the family prefixes they extend, and
dated preview or suffixed ids ride their family row. An id naming no
row resolves to nothing and passes through untouched: no clamping, no
rejection, non-fatal by design.

| Prefixes | In | Out | Thinking | Images |
|---|---|---|---|---|
| `gemini-2.0-flash`, `gemini-2.0-flash-lite` | 1,048,576 | 8,192 | no | yes |
| `gemini-2.5-flash`, `gemini-2.5-flash-lite`, `gemini-2.5-pro` | 1,048,576 | 65,536 | yes | yes |
| `gemini-3-flash`, `gemini-3.1-flash-lite`, `gemini-3.1-pro`, `gemini-3.5-flash-lite`, `gemini-3.6-flash`, `gemini-3.8-flash` | 1,048,576 | 65,536 | yes | yes |
| `gemini-2.5-flash-preview-tts`, `gemini-3.8-flash-tts` | 8,192 | 16,384 | no | no |

### The three encoder gates

Three gates fire, and only for a model the registry knows — an
unknown id keeps the ungated behavior everywhere:

- **Output clamp** — `generationConfig.maxOutputTokens` clamps down
  to the row's output cap; a request below the cap rides unchanged.
- **Thinking omit** — a thinking request on a known non-thinking
  model drops `thinkingConfig` entirely.
- **Image rejection** — an image attachment on any user or tool
  message of a known non-multimodal model is a typed `InvalidRequest`
  naming the model, raised before any wire bytes; native replay
  parts are not attachments and ride untouched.

### The Vertex mount

The Gemini dialect mounts a second way over the same encoder, decode
lane, and error taxonomy: `CRAFT_AGENT_PROVIDER=vertex` names a
Google Cloud project and a service location instead of an API key.
`GOOGLE_VERTEX_PROJECT` and `GOOGLE_VERTEX_LOCATION` are required —
each a named error when missing or empty — and the mount takes no
base-URL override: the endpoint derives from the location. An empty
project, location, or model is a typed rejection at mount, before any
wire bytes. Every request mounts `Authorization: Bearer` with an ADC
access token; `x-goog-api-key` stays exclusive to the API-key mount.

The bearer comes from the ADC credential file:
`GOOGLE_APPLICATION_CREDENTIALS` names it, else the documented gcloud
default path (`~/.config/gcloud/application_default_credentials.json`)
is used. Both documented shapes ride — a service-account key
exchanges an RS256-signed JWT at the documented token endpoint
(`grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer`), an
authorized-user key rides the refresh-token grant — under the
documented cloud-platform scope. The token cache is expiry-aware: a
cached token rides until close to its expiry, refreshes serialize
under a lock, and the credential resolves lazily on the first
request — a failed resolution names itself again on the next request
— staying cached for the process lifetime.

The stream rides the same SSE lane: the Vertex endpoint accepts
`alt=sse` (the standard Google API `alt` system parameter), so the
chunk JSON is the same generateContent schema, the error envelopes
the same `google.rpc` Status shape, and the whole error taxonomy
above — daily-quota marker, `RetryInfo` delay — applies unchanged.

## Native-block replay

Assistant messages carry `native_blocks`: dialect-tagged, verbatim
provider-native blocks (Anthropic thinking and redacted blocks, Gemini
thought signatures). Dropping them on replay is a protocol error, so they ride
the neutral message untouched and re-send verbatim — Anthropic ahead of
the turn's text and `tool_use` blocks, Gemini as leading parts. The
OpenAI dialect has no wire form: a request carrying native blocks is
rejected before any bytes leave. A block set from another dialect is
always a typed rejection, never a re-encode.

## The OpenAI accounting-frame rule

Every Chat Completions request sets `stream: true` and
`stream_options.include_usage`, so the wire emits a usage-only chunk
after `finish_reason`. That chunk is an accounting frame, not a second
terminal: the lane merges its usage and flushes one `TurnFinished`, and
the fold in `craft-core` absorbs a repeated terminal with the same stop
reason the same way — usage merges, while a different stop reason is a
corrupt stream. A turn whose usage went unreported carries the
`incomplete` flag, so downstream budget checks fail closed.
