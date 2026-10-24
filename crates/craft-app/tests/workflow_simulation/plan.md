---
name: sim-adopt
description: "Synthetic rehearsal of the judged adoption workflow's per-PR-life architecture: sweep, one detached pipeline per PR running that PR's whole life, gates, one seal."
args:
  window: { type: string, default: "7h" }
limits:
  max-steps: "10"
  max-total-turns: "2000"
  max-total-tokens: "1000000"
  max-retries-per-step: "2"
  max-concurrent-steps: "4"
steps:
  - id: prepare
    agent: sim-orchestrator
    needs: []
    max-turns: "30"
    retry: "0"
    on-failure: stop
    outputs: [collection]
    body: |
      SIM SWEEP — the scripted parallel sweep: one bash call runs the
      collection, the clone sweep, and the digest read concurrently.
      Children record local artifacts; nothing here decides anything.
  - id: pr-pipeline
    agent: sim-orchestrator
    needs: [prepare]
    max-turns: "120"
    retry: "0"
    on-failure: stop
    outputs: [pipeline-rows, filed-issues, opened-prs, pipeline-exceptions]
    body: |
      SIM PIPELINE — one pipeline per PR running that PR's WHOLE life:
      fire ONE detached spawn_agents call (profile sim-pipeline), one
      item per PR (six PRs total), carrying `detached: true`. The call
      returns at admission. Each pipeline child spawns its own stage
      children — one sim-classifier, then by class one sim-applier or
      sim-filer, then its committer after a filing lease — and polls
      ONLY its own ids. You poll ONLY the ids your spawn call named: the
      hub is shared with every pipeline's stage children, so a bulk
      drain would steal a sibling's envelope. Verify each pipeline's
      ledger row and rollup section as its envelope lands; no batch
      shapes, no coordinator.
  - id: pipeline-gate
    agent: sim-judge
    kind: judge
    needs: [pr-pipeline]
    max-turns: "40"
    retry: "1"
    on-failure: stop
    deny-write: ["**"]
    outputs: [verdict]
    judge:
      panel: "3"
      merge: majority
      max-feedback: "2"
    body: |
      GATE:pipeline — judge the per-PR output: one verdict file per PR
      under run-ledger/classifier/, at most 3 entries in each filing
      lease file under run-ledger/filing/, and one rollup section per PR
      under run-ledger/rollup/. Count them with one bash pass. Your
      final message must be exactly the verdict JSON and nothing else.
  - id: pipeline-appeal
    agent: sim-judge
    kind: judge
    needs: [pipeline-gate]
    max-turns: "40"
    retry: "1"
    on-failure: stop
    deny-write: ["**"]
    outputs: [verdict]
    judge:
      panel: "1"
      merge: unanimous
      max-feedback: "0"
    body: |
      GATE:pipeline-appeal — re-derive the pipeline-gate rubric yourself
      and return the verdict JSON as your final message.
  - id: rollup-seal
    agent: sim-orchestrator
    needs: [pr-pipeline]
    max-turns: "60"
    retry: "0"
    on-failure: stop
    outputs: [delivery-drafts]
    body: |
      SIM SEAL — compose: fire ONE spawn_agent call (profile sim-rollup)
      carrying the state snapshot built from the pipelines' per-PR
      rollup sections and the outcome rows, then report the paths.
  - id: rollup-gate
    agent: sim-judge
    kind: judge
    needs: [rollup-seal]
    max-turns: "40"
    retry: "1"
    on-failure: stop
    deny-write: ["**"]
    outputs: [verdict]
    judge:
      panel: "3"
      merge: majority
      max-feedback: "2"
    body: |
      GATE:rollup — judge the delivery drafts under run-ledger/rollup/:
      every row must cite a verified artifact. Return the verdict JSON
      as your final message.
  - id: rollup-appeal
    agent: sim-judge
    kind: judge
    needs: [rollup-gate]
    max-turns: "40"
    retry: "1"
    on-failure: stop
    deny-write: ["**"]
    outputs: [verdict]
    judge:
      panel: "1"
      merge: unanimous
      max-feedback: "0"
    body: |
      GATE:rollup-appeal — re-derive the rollup-gate rubric yourself and
      return the verdict JSON as your final message.
  - id: rollup-commit
    agent: sim-committer
    needs: [rollup-appeal, pipeline-appeal]
    max-turns: "40"
    retry: "0"
    on-failure: skip
    outputs: [delivery-rows]
    body: |
      SIM COMMIT — rollup: record the delivery outcome under
      run-ledger/outcomes/rollup.md, then report it.
---

Simulation rules: this plan is a local rehearsal of the judged adoption
workflow's per-PR-life architecture. Every artifact is a local file; no
GitHub API is called and no network is touched.
