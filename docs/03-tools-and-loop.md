# 03 — Tools and the run loop

The `Tool` trait: a name, a description, a JSON-schema parameter shape,
and an executor that emits any number of progress events followed by
exactly one terminal event. The registry caches schemas; dispatch is
dyn-erased.

v1 builtins: `bash` (timeout caps, closed stdin, SIGTERM → grace →
SIGKILL), `read_file`, `write_file` (size guard), `edit_file` (prior-read,
exact-match, and changed-on-disk guards), `glob`, `grep`, and `gh` (all
GitHub I/O shells out to the preinstalled gh CLI with GH_TOKEN).

Sensitive-file hard errors regardless of permission mode: `.env*` (except
the committed example/sample/template forms), `id_rsa*`, `credentials*`,
`.aws/`, `.gcp/`; paths escaping the workspace are rejected. The
permission posture is a per-mission tool allowlist — the digest mission
gets read + bash + gh and no repo writes.

The loop: bounded turns, per-turn usage and cost accounting against the
budget, an append-only session record (model-visible means logged), and
the stream-json NDJSON contract for headless consumers.
