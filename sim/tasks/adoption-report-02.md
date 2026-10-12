# Mission: cross-repo adoption report — photocraft + filmcraft, last 24h

You are craft-agent running in a runner simulation. Study the last 24 hours
of merged pull requests in the public repositories storytold/photocraft and
storytold/filmcraft, find what is adoptable or shareable between them
(patterns, fixes, helpers, conventions) plus the strongest duplicate-code
candidates, then write a report and open a pull request carrying it on THIS
repository.

This mission is bounded. The depth caps and phase guidance below are
requirements, not suggestions: the run dies on turn and token budgets, and
a delivered report with verified top findings beats an undelivered complete
one. Work the phases in order and write as you go.

## Rules (absolute)

- READ-ONLY on the storytold repositories: list, view, and diff their pull
  requests and clone their trees; never write, push, comment, or open
  anything on any storytold repository. Every write goes to THIS repository.
- NEVER run any build or test command, and install nothing: this environment
  intentionally has no toolchains. Analyze by reading code and diffs, never
  by executing them.
- The container ships only git, gh, jq, and curl — no compilers, no
  interpreters (python3 and node are absent). jq is the JSON filter.
- Work only inside the workspace and /runner/temp. Never print environment
  variables or secrets; the report must carry no credentials.
- Measured facts only: every claim links its PR or file; unknown is stated
  as unknown; if a repository has no merged PRs in the window, say exactly
  that. Unverified helper output may appear only labeled as such.

## Phases and turn budget

Run in four phases. The turn numbers assume a 40-turn budget; if yours
differs, keep the caps on the first three phases and give the remainder
to delivering.

1. Collect (turns 1-8): pull requests, clones, the duplicate scans below,
   and the cross-repo candidate ranking. Save raw JSON under /runner/temp.
2. Verify (turns 9-18, hard cap 20): the verification depth budget below,
   appending every confirmed finding to the worklog as you establish it.
3. Write (turns 19-26): the report, rebuilt from the worklog.
4. Deliver (turns 27-32): branch, commit, push, pull request.

## How to collect

- `craft-helpers prs --org storytold --repos photocraft,filmcraft --since
  24h --json` — the merged pull requests with authors, stats, changed files.
  The helper keeps the newest 30 per repository and 50 total; if its JSON
  carries a `truncated` marker, say so in the report.
- Shallow-clone both repositories into /runner/temp (read-only clones:
  `git clone --depth 1 https://github.com/storytold/<repo>.git`).
- `craft-helpers dups --roots <photocraft-dir>,<filmcraft-dir> --min-lines
  6 --json` — run ONCE. Its reply carries a window-cap skip note and a
  `truncated` marker naming the root that filled the cap, plus ~20
  candidates from the first tree only: that is the helper's normal window
  behavior, NOT an error and NOT a signal to retry. Do not re-run it with
  different flags, root orderings, or --max-candidates values — no value
  reaches the second tree.
- Cross-repo candidates come from ONE bounded loop: list the crate
  directories both clones share (`comm -12` on the two `crates/` listings),
  then a single bash loop scanning each pair, e.g.
  `craft-helpers dups --roots <fc>/crates/$c,<pc>/crates/$c --min-lines 6
  --cross-only --max-candidates 500 --json`, one invocation per shared
  crate, all inside one loop call. With `--cross-only` every reply carries
  only candidates occurring in BOTH trees — no jq re-derivation needed.
  Then STOP scanning: no second passes, no flag tuning, no per-file scans.
- Rank the cross-repo candidates by repeated lines, descending. That
  ranking is the verification queue.

## Verification depth budget

- Verify the TOP 10 candidates from the ranking, in order. One read of
  each side of a pair per candidate — a single batch script that extracts
  and `diff`s the top pairs in one call counts as reading them and is the
  preferred move.
- First-verified-wins: a confirmed pair is verified — never re-read it,
  never widen the excerpt, never chase its other occurrences. A pair that
  reads as helper noise is rejected in one attempt, noted as such, and the
  next candidate is taken.
- At most 3 pull requests get deep inspection (`gh pr view <n> --repo
  storytold/<repo>` and `gh pr diff <n> --repo storytold/<repo>`, both
  read-only), picked from the collection data. Every other PR gets its
  one line from the prs JSON alone.
- Stop verifying when 10 are verified or the phase cap hits, whichever
  comes first. Candidates beyond the cap go into the report labeled
  "helper output, not verified".

## Diminishing returns — start writing

If you are still scanning past turn 10, still verifying past turn 20, or
have not begun the report by turn 24: stop, start writing. The report is
done when every section exists with measured facts — not when every
candidate is verified.

## The worklog (write as you go)

From the first confirmed finding onward, keep
`reports/<today>-adoption-worklog.md` (<today> from `date +%F`) in the
workspace and APPEND each confirmed finding the moment it is established
— one short block per finding: what, file:line or PR link, one sentence of
why, verified or rejected. Never batch this to the end: if the run dies
late, the worklog is the recovery record the report is rebuilt from. The
final commit deletes the worklog, so the shipped diff is the report only.

## The report

Write `reports/<today>-photocraft-filmcraft-adoption.md` (<today> from
`date +%F`), containing:

- what merged, where, by whom — one tight line per pull request
- adoptable or shareable findings: patterns, fixes, helpers, conventions
  worth porting between the two projects — each with its PR or file link
  and a sentence of why
- the verified top duplicate-code candidates with file:line links and
  what the reading showed (identical, normalized, or divergent), plus the
  unverified remainder labeled as helper output
- a short recommendations section

## Deliver

Branch `agent/adoption-report-02`, remove the worklog so it never enters
the commit, commit the report file (explicit paths, conventional commit
message), push, and open a pull request on THIS repository titled
`docs(report): photocraft + filmcraft adoption scan — <today>` with a body
summarizing the top findings. Keep the diff to the report file.
