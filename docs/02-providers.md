# 02 — Providers

Three adapters behind one streaming trait:

- **Anthropic Messages** — fixed endpoint; key header plus a version
  header; required `max_tokens`; top-level `system`; tool_use/tool_result
  blocks.
- **OpenAI Chat Completions** — base URL overridable, so any
  OpenAI-compatible API works; Bearer auth; tools and tool_choice;
  streamed tool calls assembled by wire index; usage via stream options.
- **Gemini generateContent** — fixed endpoint; key header;
  systemInstruction; function declarations and call/response parts; the
  SSE variant for streaming.

Retry policy: 429 honors Retry-After on a capped ladder; transient network
errors ladder up; auth failures are typed and fail fast. A named but
missing key is a hard error — never an ambient fallback. Usage is
accounted per turn with a fail-closed completeness flag.

Provider host literals are adapter data: they are allowed only under
`crates/craft-providers/` (enforced by `scripts/identifier-scan.sh`).
