# Mission: a six-part analysis the token budget must cut short

You are craft-agent running in a runner simulation. This repository
is craft-agent itself. Produce a six-part analysis of this
repository's workspace guard and exit contract, one part per
assistant turn, each part substantial. The run's token budget is far
smaller than the work demands — the run is expected to die when the
cumulative ledger crosses the cap; that is this fixture's purpose.

## The six parts

1. The sensitive-path guard: which paths are denied, and workspace
   containment (crates/craft-tools/src/pathguard.rs).
2. The exit-code taxonomy: 0/1/2/3/4 and their meanings
   (crates/craft-runtime/src/outcome.rs).
3. How a denied tool call aborts the whole run
   (crates/craft-runtime/src/call.rs).
4. How the token ledger is checked after every turn
   (crates/craft-runtime/src/budget.rs).
5. How the sim container maps exit codes to conclusions
   (sim/entrypoint.sh).
6. What each guard and exit path exists to protect in production
   (docs/01-architecture.md).

## Cadence (absolute)

- Produce exactly ONE part per assistant turn, in order, grounded in
  the named file. Never deliver two parts in one turn.
- Each part is at least 350 words of real analysis: no padding, no
  summaries, no bullet-list shortcuts.
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

    CRAFT_AGENT_MAX_TOKENS=4000 sim/run-sim.sh budget-01

The knob passes through to the container: six substantial parts with
a context that grows every turn must push cumulative usage — input,
output, and cached tokens — past 4000 within a few turns.

## Expected conclusion (operator)

Exit 3, `conclusion=budget`. With `<run>` the new sim/runs entry:

- `grep '^conclusion=budget$' <run>/out/outputs.env`
- `grep run_finished <run>/out/stream.jsonl` — the line carries
  `"exit_code":3`, `"category":"budget"`, and a detail starting
  `token budget exceeded` that names the spend against the cap.
- `git -C <run>/workspace log --oneline -1` unchanged: no commit.
