Mission: open a small documentation pull request to this repository.

This repository is craft-agent itself. Add a short `sim/README.md` that
documents the runner simulation shipped in the `sim/` directory.

The README must contain:
- a title for the simulation,
- one paragraph explaining the purpose: a Docker container that behaves
  like a GitHub Actions runner — it installs the checksum-verified musl
  release binary from the mounted dist/, gets a fresh clone of this
  repository as its workspace, and runs one craft-agent mission end to
  end,
- how to run it: `sim/run-sim.sh [task]` builds the release binary,
  builds the image, clones a fresh workspace, and runs the prompt at
  `sim/tasks/<task>.md`.

Hard constraints:
- Work only inside this workspace. Never touch any other repository, and
  never call gh against the storytold organization.
- Create the branch `agent/open-pr-01`, commit with a conventional
  commit message, and stage explicit paths only — never `git add -A`.
- Push the branch and open a pull request to THIS repository with a
  clear title and body.
- Keep the diff minimal: one new file, no other edits.
