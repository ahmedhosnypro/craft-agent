# Runbook — the craft-agent GitHub App

The digest workflow acts as `craft-agent[bot]` through a GitHub App
token: each job mints its own token, narrowed to what that job needs,
and the token is revoked automatically when its minting job ends. The
App is the installation that grants; the per-job mints only narrow.

## Create the App

1. Organization settings → Developer settings → GitHub Apps →
   New GitHub App.
2. Name it exactly **craft-agent**. The bot login becomes
   `craft-agent[bot]`, and the digest workflow locates its own issues
   with `author:app/craft-agent` — a different name breaks the sticky
   issue search.
3. Leave webhooks inactive; the digest polls nothing.
4. Repository permissions:
   - **Contents: Read-only**
   - **Issues: Read & write**
5. Create the App, copy the **Client ID** shown on its page, and
   generate a private key (the downloaded `.pem` file).

The installation must carry at least those two permissions. A mint
requesting a permission the installation lacks fails the mint — the run
goes red, which is the correct loudness for a misconfiguration.

## Install the App

Install on the **storytold** organization, either on all repositories or
on `craft-agent` plus every repository listed in
`config/digest-repos.txt`. The digest token is minted with the same
repository pin, so a repository the installation does not cover makes
the mint fail before anything runs. All-repositories is the
low-maintenance choice; the per-mint `repositories` pin still narrows
each token.

Re-creating the App (new client id, new bot identity) orphans the
existing sticky issue — the searches no longer match it. Close the old
issue manually; the next run creates a fresh one and the 24h window
fallback covers the gap.

## Store the credentials

Add two repository secrets on `storytold/craft-agent`
(see `docs/runbook-secrets.md` for the full checklist):

- `CRAFT_AGENT_APP_CLIENT_ID` — the Client ID from the App page.
- `CRAFT_AGENT_APP_PRIVATE_KEY` — the whole `.pem` file contents,
  including its header and footer lines.

## Verify

1. Dispatch **suite-digest** with `dry-run` on: the mint succeeds, the
   digest runs over the org, nothing is published. A red job here is
   almost always an installation-scope or secret problem; the log line
   names the missing piece.
2. Dispatch without `dry-run`: the publish job creates the sticky issue
   `Suite digest` under `craft-agent[bot]` and its body ends with the
   window marker.
3. Dispatch once more: the window now derives from that marker.

## Disable or roll back

- Disable the schedule by setting `DIGEST_ENABLED` to any value other
  than `true` (manual dispatches keep working).
- Suspend or uninstall the App to stop all activity; every mint then
  fails red. Do both when retiring the digest — the sticky issue can be
  closed at any time; a later run finds it, updates it, and reopens it.
