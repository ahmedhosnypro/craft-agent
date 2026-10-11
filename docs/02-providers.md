# 02 — Providers

Three wire adapters — Anthropic Messages, OpenAI Chat Completions,
Gemini generateContent — implement one streaming `Provider` trait over
the neutral vocabulary of `craft-core`; all three are clean-room
implementations of the providers' public documentation. Wire
differences live in the adapters, never in the callers: a `Backend`
enum mounts exactly one dialect and the runtime dispatches through it
without knowing which wire it is talking to.

## Constructor seam

An adapter is constructed with three explicit values — base URL, model
id, API key — and nothing else. No adapter reads the environment; the
app layer resolves configuration and mounts the backend, so a lane or
corpus can mount a loopback endpoint without touching process state.
The base URL joins the endpoint path verbatim (a trailing slash is
trimmed), so the request itself never carries endpoint state.

## BYOK environment

The app layer (`craft-app` config) reads named variables only — a
missing or empty variable is a hard error that names itself; there is no
ambient fallback:

| Variable | Role |
|---|---|
| `CRAFT_AGENT_PROVIDER` | Required. Selects the dialect: `openai`, `anthropic`, or `gemini`; anything else is a typed rejection. |
| `CRAFT_AGENT_MODEL` | Required. The model id sent to the provider. |
| `OPENAI_API_KEY` | Required for the `openai` dialect. |
| `OPENAI_BASE_URL` | Optional override of the OpenAI base URL, so any OpenAI-compatible lane works; default `https://api.openai.com/v1`. |
| `ANTHROPIC_API_KEY` | Required for the `anthropic` dialect. |
| `GOOGLE_API_KEY` | Required for the `gemini` dialect. |
| `GH_TOKEN` | Optional; handed to the gh tool. |

The binary reads only the key matching the selected provider and
removes provider-key variables from every child process.

## Endpoints and authentication

| Dialect | Endpoint | Authentication |
|---|---|---|
| OpenAI Chat Completions | `{base}/chat/completions` | `Authorization: Bearer <key>` |
| Anthropic Messages | `{base}/v1/messages` | `Authorization: Bearer <key>` plus `anthropic-version: 2023-06-01` |
| Gemini generateContent | `{base}/v1beta/models/{model}:streamGenerateContent?alt=sse` | `x-goog-api-key: <key>` |

Every request carries `Content-Type: application/json`. Default hosts:
`https://api.openai.com/v1` (the version segment is part of the base,
so an override stays fully shaped), `https://api.anthropic.com`, and
`https://generativelanguage.googleapis.com` (the adapter appends the
version and model path itself, so an override stays host-shaped). A
non-2xx body is classification input only and is read capped at 64 KiB,
so a hostile server cannot balloon the process. Provider host literals
are adapter data: they are allowed only under `crates/craft-providers/`
(enforced by `scripts/identifier-scan.sh`).

## The retry ladder

One shared ladder (`craft-core` constants), mounted identically by all
three adapters:

| Constant | Value | Meaning |
|---|---|---|
| `RETRY_MAX_ATTEMPTS` | 6 | Attempts per request, connection failures only. |
| `RETRY_BACKOFF_INITIAL_SECS` | 2 | First backoff step; doubles each attempt. |
| `RETRY_BACKOFF_CAP_SECS` | 30 | Backoff ceiling; a live `Retry-After` is clamped to it. |
| `RATE_LIMIT_STREAK` | 5 | Consecutive 429s that surface the `RateLimit` error immediately. |

Only `RateLimit`, `Transient`, and `Transport` retry: authentication and
client errors fail fast. A `Retry-After` header (delta-seconds form
only; HTTP-date and junk fall back to the ladder) overrides the
computed backoff, clamped to the cap.

## The decode lane

All three adapters share one lane shape — the buffered-terminal
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
  arguments; `thinking` and future block kinds register but never
  surface. A delta on the wrong block kind, or without a start, is a
  typed transport error.
- **Gemini** — camelCase chunk JSON; `usageMetadata` merges
  (`thoughtsTokenCount` lands in `reasoning_tokens`); a
  `promptFeedback.blockReason` is a `ContentFilter` terminal; thought
  parts and bare signatures never reach the turn; a `functionCall`
  part lands whole on the next adapter-assigned index.

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
(carrying the live `Retry-After`), `Transient`, `PayloadTooLarge`,
`ContextWindow`, `InvalidRequest`, `Transport`, `IdleTimeout`,
`StreamClosed`, and `EmptyResponse`. HTTP classification is shared:
401/403 → `Auth`; 429 → `RateLimit`; 413 → `PayloadTooLarge`; 5xx and
408 → `Transient`; other 4xx → `InvalidRequest`; anything else →
`Transport`. Body-driven refinements run before the taxonomy closes —
a context-window rejection riding a plain 400 is `ContextWindow`:

- OpenAI: an error code or message containing `context_length` /
  `context length`.
- Anthropic: a message containing `context window` or
  `prompt is too long`.
- Gemini: a message containing `context window`.

Mid-stream error events (a chunk or named event carrying an error
envelope) map onto the same taxonomy:

| Wire kind | Taxonomy |
|---|---|
| OpenAI `rate_limit`, code `insufficient_quota` | `RateLimit` |
| OpenAI `server_error`, `service_unavailable` | `Transient` |
| OpenAI `invalid_request_error` | `InvalidRequest` |
| OpenAI `authentication_error`, `permission_error` | `Auth` |
| Anthropic `rate_limit_error` | `RateLimit` |
| Anthropic `overloaded_error`, `api_error`, `timeout_error` | `Transient` |
| Anthropic `invalid_request_error`, `not_found_error` | `InvalidRequest` |
| Anthropic `authentication_error`, `permission_error` | `Auth` |
| anything else | `Transport` |

## Request encoding

- **OpenAI** — the system prompt rides as a leading system message;
  tools ride as `function` entries with `tool_choice: auto`.
- **Anthropic** — top-level `system`; `max_tokens` is required; no
  temperature rides the body (the dialect deprecated it). Assistant
  turns are content blocks — `text` and `tool_use` (arguments parse
  into the input object the wire requires). Consecutive tool results
  flush as one user message of `tool_result` blocks; the error flag
  marks rows the tool layer refused.
- **Gemini** — `systemInstruction` parts;
  `generationConfig.maxOutputTokens`; tools as `functionDeclarations`
  with `parametersJsonSchema`; assistant turns are `model` parts. A
  `functionResponse` requires the function name, resolved from the
  assistant call it answers — a result with no pairing call is a typed
  rejection. An error result rides the documented error key; a
  JSON-object result rides verbatim; any other text wraps in `output`.

## Native-block replay

Assistant messages carry `native_blocks`: dialect-tagged, verbatim
provider-native blocks (Anthropic thinking blocks, Gemini thought
signatures). Dropping them on replay is a protocol error, so they ride
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
