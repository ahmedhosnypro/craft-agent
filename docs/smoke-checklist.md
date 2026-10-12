# Smoke checklist — in-repo action verification

`smoke.yml` proves the composite action end to end on a real runner,
keylessly: no secret, provider key, or repository variable is read by any
step. Every leg asserts the typed fail-closed contract the binary
promises when credentials are absent — never model success.

## Preconditions

- The workflow is dormant: it runs only when hand-dispatched, so authoring
  it is safe before anything else exists.
- A release with the `craft-agent-<triple>` and
  `craft-helpers-<triple>` binaries and `checksums.txt`
  assets must exist in this repository (`scripts/build-release.sh` builds
  them). Dispatch with `version` set to the exact tag until the moving
  `v1` tag points at a release; the default is `v1`.
- The install step downloads with unauthenticated curl, so the repository
  must be public for the release download to succeed.
- No secrets are required or read. If any were set, the smoke would still
  pass — the probes strip the provider env with `env -u` and the action
  legs pass empty provider values.

## Dispatching

Actions tab → **smoke** → **Run workflow** → pick the branch → set
`version` if not `v1` → **Run**. The job must finish in well under a
minute; `timeout-minutes: 10` bounds any hang.

## What each leg proves

1. **Checkout** — fetches the tree the action runs from; the only pinned
   third-party action, `actions/checkout` at the full SHA verified for the
   other workflows.
2. **Run the action with no provider** — exercises `uses: ./` with
   `provider: ""`, so the mission step inherits an absent
   `CRAFT_AGENT_PROVIDER`. The install step must download the release
   binary and verify its checksum; the run mission must then stop before
   any model call.
3. **Assert the keyless run failed typed** — the step outcome must be
   `failure` and the action's `conclusion` output must be `error`. An
   empty `conclusion` means the install step failed — no release at the
   requested tag — and the smoke is red, not silently green.
4. **Binary sanity** — `craft-agent --help` and `--version` against the
   installed binary both exit 0.
5. **Probe the no-provider error** — the binary run directly with the
   provider env stripped must exit 1 and print
   `CRAFT_AGENT_PROVIDER is required but not set`.
6. **Probe the missing-key error** — `digest --dry-run` with a provider
   set and every provider key stripped must exit 1 and print
   `OPENAI_API_KEY is required but not set`, proving the per-provider
   key-name mapping.

7. **Digest dry run through the action** — the action again, `mission:
   digest` with `--dry-run` in `args`, provider set, no key mapped. The
   digest preflight must stop at the same typed env error; the pipeline
   behind it is never reached keylessly.
8. **Assert the keyless digest failed typed** — outcome `failure`,
   `conclusion` output `error`.

## What to observe in a green run

- Both action legs show the install step's checksum verification, then
  the mission step's `craft-agent exit code 1` trailer with the typed
  error line naming the missing variable.
- Both `conclusion` outputs are `error` in the step summaries; the
  `final-message` outputs are empty (no model call was made).
- The probe steps print nothing on success; their `test`/`grep`
  assertions are the proof.

## Follow-ups after a green dispatch

- Publish the release: build the musl binaries, attach the two binaries
  plus `checksums.txt` to the semver tag, and move the `v1` tag to it.
- Re-dispatch the smoke with the default `version: v1` to prove the
  moving tag resolves to the just-published release.
- External-consumer readiness: another repository can now use the action
  pinned at `uses: ahmedhosnypro/craft-agent@v1` (docs tell consumers to
  full-SHA-pin) with its own provider key mapped in step `env` — the
  in-repo smoke having proven the download, checksum, mission, and
  output contract it will rely on.
