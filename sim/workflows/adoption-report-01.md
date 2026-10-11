---
name: adoption-report-01
description: Photocraft + filmcraft adoption scan over 24h of merged PRs.
args:
  window: { type: string, default: 24h }
  repos: { type: array, default: [photocraft, filmcraft] }
limits:
  max-steps: 6
  max-total-turns: 60
  max-total-tokens: 4000000
  max-retries-per-step: 1
steps:
  - id: collect
    agent: repo-scout
    model: glm-5.3-flash
    needs: []
    max-turns: 12
    retry: 1
    on-failure: stop
    outputs: [pr-inventory]
    body: |
      Run `craft-helpers prs --org storytold --repos photocraft,filmcraft
      --since {{arg.window}} --json` and write the JSON inventory to
      /runner/temp/pr-inventory.json. The call is read-only against the
      storytold organization; every write in this step lands in the
      workspace or /runner/temp. Final message: one tight line per merged
      pull request — repo, number, title, author — or, for a repository
      with none in the window, exactly that fact.
  - id: analyze
    agent: pr-analyst
    model: glm-5.3-flash
    needs: [collect]
    max-turns: 15
    retry: 1
    on-failure: stop
    outputs: [adoption-findings]
    body: |
      Read /runner/temp/pr-inventory.json and turn the collected JSON
      into findings. Inspect interesting pull requests with `gh pr view`
      and `gh pr diff` (both read-only), shallow-clone both repositories
      into /runner/temp, and run `craft-helpers dups --min-lines 6
      --json`; verify every duplicate candidate by reading the code.
      Final message: the adoptable or shareable findings — patterns,
      fixes, helpers, conventions worth porting between the two
      projects, each with its PR or file:line link — plus the verified
      duplicates.
  - id: deliver
    agent: docs-writer
    model: glm-5.3-flash
    needs: [analyze]
    max-turns: 15
    retry: 1
    on-failure: stop
    outputs: [adoption-report, adoption-pr]
    body: |
      Write `reports/<today>-photocraft-filmcraft-adoption.md` in the
      workspace (`<today>` from `date +%F`): what merged, where, by
      whom; the adoptable findings; the verified duplicates; a short
      recommendations section. Then branch `agent/adoption-report-01`,
      commit the report file with a conventional commit message,
      staging explicit paths only, push, and open the pull request on
      THIS repository. Keep the diff to the report file.
---

# Rules (absolute) — prepended to every step

- READ-ONLY on the storytold repositories: list, view, diff, and clone
  their pull requests and trees; never write, push, comment, or open
  anything on any storytold repository. Every write goes to THIS
  repository.
- All writes to THIS repository only: the report, the branch, the
  commit, the pull request — nothing else, nowhere else.
- Measured facts only: every claim links its PR or file:line; unknown
  is stated as unknown.
- Never run builds or tests, and install nothing: analyze by reading
  code and diffs, never by executing them.
- Work only inside the workspace and /runner/temp. Never print
  environment variables or secrets; no report or message carries
  credentials.
