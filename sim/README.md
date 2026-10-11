# The runner simulation

`sim/` ships a Docker container that behaves like a GitHub Actions
runner — the image installs the checksum-verified musl release binary
from the mounted `dist/`, receives a fresh clone of this repository as
its workspace, and runs one craft-agent mission end to end, the way the
composite Action does against a real runner. It exists so the agent can
be exercised against a live gateway, a real repo, and the full
install-run-report cycle without waiting on a workflow dispatch; no
secrets and no repo source are baked into the image, and the run leaves
its stream, outputs, and log under `sim/runs/<timestamp>/`.

## Run it

`sim/run-sim.sh [task]` builds the release binary, builds the image,
clones a fresh workspace, and runs the prompt at
`sim/tasks/<task>.md` — defaulting to `open-pr-01`:

```sh
./sim/run-sim.sh open-pr-01
```

Prerequisites: `docker`, host `gh` with an authenticated token (or
`GH_TOKEN_FILE`), and a `.env` at the repo root carrying
`TEST_BYOK_API_KEY` and `TEST_BYOK_BASE_URL` — see `.env.example`.
