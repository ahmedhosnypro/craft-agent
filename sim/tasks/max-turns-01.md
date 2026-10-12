# Mission: a five-part analysis the turn cap must cut short

You are craft-agent running in a runner simulation. This repository
is craft-agent itself. Produce a five-part analysis of the runner
simulation shipped in `sim/`, one part per assistant turn —
deliberately more turns than the run's turn cap allows. The run is
expected to die at the cap; that is this fixture's purpose.

## The five parts

1. What the simulation is for: the gap between an Actions runner and
   the local container (sim/entrypoint.sh).
2. How a run is staged: build, image build, fresh clone, mounts
   (sim/run-sim.sh).
3. How the mission input reaches the binary: prompt file and env
   knobs (sim/entrypoint.sh).
4. How an exit code becomes the conclusion line in outputs.env
   (sim/entrypoint.sh).
5. Why a capped run must conclude `max-turns`, never `success`.

## Cadence (absolute)

- Before any part, call bash once to print the current date — the
  first assistant turn must contain that call and nothing else.
- Produce exactly ONE part per assistant turn, in order, grounded in
  the named file. Never deliver two parts in one turn, never skip
  ahead, never compress the parts into a summary.
- End each part with the marker line `— end of part N —` and stop
  the turn; the next turn delivers the next part.

## Rules (absolute)

- Work only inside this workspace. Never touch any other repository,
  and never call gh against the storytold organization.
- This mission expects no file writes at all: parts are assistant
  text. No branch, no commit, no pull request — partial work stays
  uncommitted in the workspace.
- Never print environment variables or secrets.

## How this mission is run (operator)

    CRAFT_AGENT_MAX_TURNS=1 sim/run-sim.sh max-turns-01

The knob passes through to the container: one turn allowed, and the
mission's first action is a tool call that structurally requires a
second turn — the run must die at the cap.

## Expected conclusion (operator)

Exit 2, `conclusion=max-turns`. With `<run>` the new sim/runs entry:

- `grep '^conclusion=max-turns$' <run>/out/outputs.env`
- `grep run_finished <run>/out/stream.jsonl` — the line carries
  `"exit_code":2`, `"category":"max_turns"`, and the detail
  `run aborted after max turns (1)`.
- `git -C <run>/workspace log --oneline -1` still names the clone's
  original HEAD: no commit, no branch, no pull request.
