# 05 — The GitHub Action

`action.yml` is a composite action: inputs (provider, model, mission
arguments, budget, prompt), a pinned release-binary download
(checksum-verified) for the runner triple, an invocation of
`craft-agent digest|run`, and outputs (digest path, final message).

The digest workflow (daily `schedule` plus `workflow_dispatch`) mints the
GitHub App token, checks out with full history, installs the copy-paste
detector, runs the action with the BYOK secret, then publishes in a second
job. Live integration corpora run on runners only through a
`workflow_dispatch`-only workflow with an empty-secret preflight hard-fail
— never on pull-request events.

Releases are semver tags plus a moving `v1`; consumers pin by full commit
SHA.
