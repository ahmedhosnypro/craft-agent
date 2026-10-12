# Mission: reply with one word while an invalid model id kills the run

You are craft-agent running in a runner simulation. This repository
is craft-agent itself. Your entire task is to reply with exactly one
word: `pong`. No tools, no file reads, no exploration — the mission
is deliberately trivial so that the model id configured for the run
is the only possible failure source.

## Rules (absolute)

- Work only inside this workspace. Never touch any other repository,
  and never call gh against the storytold organization.
- This mission expects no writes at all, no branch, no commit, no
  pull request.
- Never print environment variables or secrets.

## How this mission is run (operator)

    CRAFT_AGENT_MODEL=nonexistent-model-probe sim/run-sim.sh error-01

The model id does not exist at the gateway, so the first provider
call fails with a typed provider error and the run aborts. Note:
run-sim.sh forwards the turn and token knobs but currently
hardcodes the model id, so it needs the same one-line passthrough
for CRAFT_AGENT_MODEL before this mission can run as designed.

## Expected conclusion (operator)

Exit 1, `conclusion=error`. With `<run>` the new sim/runs entry:

- `grep '^conclusion=error$' <run>/out/outputs.env`
- `grep run_finished <run>/out/stream.jsonl` — the line carries
  `"exit_code":1`, `"category":"error"`, and the provider's typed
  error message as the detail.
- The stream shows no assistant text: the model was never reachable.
