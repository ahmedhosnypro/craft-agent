# Mission: cross-repo adoption report — photocraft + filmcraft, last 24h

You are craft-agent running in a runner simulation. Study the last 24 hours
of merged pull requests in the public repositories storytold/photocraft and
storytold/filmcraft, find what is adoptable or shareable between them
(patterns, fixes, helpers, conventions) plus duplicate-code candidates, then
write a report and open a pull request carrying it on THIS repository.

## Rules (absolute)

- READ-ONLY on the storytold repositories: list, view, and diff their pull
  requests and clone their trees; never write, push, comment, or open
  anything on any storytold repository. Every write goes to THIS repository.
- NEVER run any build or test command, and install nothing: this environment
  intentionally has no toolchains. Analyze by reading code and diffs, never
  by executing them.
- Work only inside the workspace and /runner/temp. Never print environment
  variables or secrets; the report must carry no credentials.

## How to collect

- `craft-helpers prs --org storytold --repos photocraft,filmcraft --since
  24h --json` — the merged pull requests with authors, stats, changed files.
- `gh pr view <n> --repo storytold/<repo>` and `gh pr diff <n> --repo
  storytold/<repo>` (both read-only) for deeper inspection of interesting PRs.
- Shallow-clone both repositories into /runner/temp (read-only clones:
  `git clone --depth 1 https://github.com/storytold/<repo>.git`), then
  `craft-helpers dups --roots <photocraft-dir>,<filmcraft-dir> --min-lines 6
  --json` for duplicate-code candidates.

## The report

Write `reports/<today>-photocraft-filmcraft-adoption.md` in the workspace
(`<today>` from `date +%F`), containing:

- what merged, where, by whom — one tight line per pull request
- adoptable or shareable findings: patterns, fixes, helpers, conventions
  worth porting between the two projects — each with its PR or file link
  and a sentence of why
- duplicate-code candidates from the helper, verified by actually reading
  the code, with file:line links
- a short recommendations section

Measured facts only: every claim links its PR or file; unknown is stated as
unknown; if a repository has no merged PRs in the window, say exactly that.

## Deliver

Branch `agent/adoption-report-01`, commit the report file (explicit paths,
conventional commit message), push, and open a pull request on THIS
repository titled `docs(report): photocraft + filmcraft adoption scan —
<today>` with a body summarizing the top findings. Keep the diff to the
report file.
