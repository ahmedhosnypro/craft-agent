# Runbook — secrets, variables, enable and rollback

Everything the workflows read from the environment lives in repository
settings on `ahmedhosnypro/craft-agent`: **Actions secrets** for values
that must never print, and **Actions variables** for plain
configuration. Set
values through the settings UI; never paste a value into an issue, a
dispatch input, or a log.

## Secrets

| Secret | Used by | Value |
|---|---|---|
| `CRAFT_AGENT_APP_CLIENT_ID` | suite-digest, release | The GitHub App Client ID. |
| `CRAFT_AGENT_APP_PRIVATE_KEY` | suite-digest, release | The App private key file contents, including header and footer lines. |
| `OPENAI_API_KEY` | suite-digest | Only when `CRAFT_AGENT_PROVIDER` is `openai`. |
| `ANTHROPIC_API_KEY` | suite-digest | Only when `CRAFT_AGENT_PROVIDER` is `anthropic`. |
| `GOOGLE_API_KEY` | suite-digest | Only when `CRAFT_AGENT_PROVIDER` is `gemini`. |
| `TEST_BYOK_BASE_URL` | integration-live | Gateway base URL, including the `/v1` prefix. |
| `TEST_BYOK_API_KEY` | integration-live | Gateway key for the live corpora. |
| `TEST_BYOK_MODEL` | integration-live | Model id the live corpora request. |

The provider key for the digest is whichever one matches
`CRAFT_AGENT_PROVIDER` — the other two may stay unset; a named-but-empty
key for the chosen provider is a hard error naming the variable. The
integration-live workflow hard-fails with a `::error::` naming any empty
`TEST_BYOK_*` secret; locally the same corpora SKIP when `.env` is
absent, so the runner behavior is stricter on purpose.

## Variables

| Variable | Used by | Value |
|---|---|---|
| `DIGEST_ENABLED` | suite-digest | `"true"` lets scheduled runs proceed; anything else keeps cron dormant. Dispatch always runs. |
| `CRAFT_AGENT_PROVIDER` | suite-digest | `anthropic`, `openai`, or `gemini`. |
| `CRAFT_AGENT_MODEL` | suite-digest | The digest model id — it must support tool calling. |

## Enable

1. Create the App and store its credentials
   (see `docs/runbook-app.md`).
2. Add the provider key secret matching the chosen provider, and set the
   two `CRAFT_AGENT_*` variables.
3. Dispatch **suite-digest** once with `dry-run` to verify the mint and
   a full collection.
4. Set `DIGEST_ENABLED` to `"true"`. The next cron run publishes the
   first sticky issue.

## Disable and roll back

- Set `DIGEST_ENABLED` to `"false"` (or delete it) to stop the daily
  runs without touching anything else. Manual dispatches keep working —
  use them for a deliberate run.
- To stop spending entirely: disable the schedule, then suspend or
  uninstall the App.
- Coverage is never lost while disabled: each run derives its window
  from the sticky issue's window marker, and a missing or malformed
  marker falls back to 24h, clamped to 30 days — the first run after a
  gap re-collects it.

## Keepalive for the schedule

GitHub disables a scheduled workflow after roughly 60 days without
repository activity. Any push to the repository resets that clock, and a
manual dispatch counts as a run — a recurring calendar reminder to
dispatch **suite-digest** (or merge any change) keeps the cron alive.
If the schedule was disabled anyway, re-enable the workflow from the
Actions tab; the window marker self-heals the missed coverage on the
first run.

## Hygiene

- The secret scanners run on every commit; a committed `.env` or a pasted
  key is a red gate. Fix by rotating the exposed credential first.
- The provider key never reaches child processes of the agent, artifacts,
  or the published issue: the digest is scrubbed before it leaves the
  binary, and the publish job runs without any provider key in reach.
