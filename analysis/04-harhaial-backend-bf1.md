
## 6. Reusable patterns for the new agentic GitHub Action

Ranked by direct applicability to "agent as GitHub Action over ~22 Rust app repos, heavy
maintainer PR review, strict verification gates, BYOK":

1. **One gate script, invoked everywhere** (gate ↔ pre-commit ↔ CI mirror). The 12-stage
   `scripts/quality-gate.sh` is the single definition of done, and ci.yml documents stage
   parity in its header. For the storytold agent this is the backbone: the agent's PR
   passes the SAME gate the maintainer runs locally — the agent can even run it before
   pushing. Directly reusable stages for pure-Rust app repos: fmt, clippy `-D warnings` with
   centralized `[workspace.lints]`, nextest, `cargo deny`, `cargo machete`, build `--locked`.
2. **Fail-closed gate-before-fetch pipelines** (provider-proxy.md "The Pattern"): every
   check that can refuse runs BEFORE any expensive/irreversible action, each refusal typed
   with one error shape. Maps 1:1 to agent-run admission: entitlement/budget check → spawn;
   a denied run costs nothing. The entitlement law ("an unprovable balance BLOCKS; nothing
   falls open") is the right posture for an autonomous agent's kill switches.
3. **Reserve-and-Settle budgeting for BYOK**: hold credits before the LLM call, settle
   exactly once however the run dies (crash, timeout, doom-loop), never-negative by
   construction; meter metadata only (model, token counts, reasons — never content, never
   keys). If the storytold agent meters BYOK spend per job, this escrow design is a
   ready-made law. Provider keys "only in server-side secret storage" (AD-010) = GitHub
   Actions secrets/org-level secrets; the canary-selftest secrets scanner (with `sk-`,
   `sk-ant-`, `AIza` patterns) is what keeps agent-written code from leaking them.
4. **The security-audit-pool orchestration blueprint** is the closest existing design for
   the new project itself: event → spawn a bounded worker pool → **SQLite DB as the only
   coordination medium** (runs/claims/events rows) → watchdog + liveness probes + recovery
   recipes → dedup/triage/severity → review output → optional test-first remediation →
   serialized gate + explicit-path commits. Its experience rules (claims as advisory locks
   via UNIQUE violations, prompt files + vars never inline prose, ban the git write path in
   workers, verify-before-flag with file:line citations, self-learning INDEX that
   supersedes defaults) read like a spec for a robust GitHub-driven agent fleet.
5. **Write-time advisory hooks**: the `postToolUse` → `additionalContext` loop (advisory,
   fail-open, same checker as the gate) translates to the Action leaving inline PR review
   comments the moment a rule trips, while enforcement remains the merge-blocking CI gate.
   Two-tier responsibility (feedback vs. enforcement) avoids the hook-blocks-the-agent
   failure mode.
6. **Event handling without a queue**: no apalis/redis — webhooks are synchronous routes
   with replay-dedup + `Idempotency-Key` on every money/security mutation, and periodic work
   is in-process tokio sweepers. A GitHub Action gets its queue for free (event-driven
   workflow runs); what to copy is the **idempotency law** — GitHub redelivers webhooks, so
   every event handler needs replay-dedup keyed the same way.
7. **Runtime self-protection**: per-provider circuit breaker (open → answer locally, never
   queue), doom-loop kill (N identical outputs → terminate + meter), three-phase timeouts
   where keepalives never reset the watchdog. For an agent doing long LLM generations these
   are the exact runaway guards (the user has also seen runaway loops in Plasma QML — same
   class of failure).
8. **Testing infra**: scratch-DB two-axis distinctness (test DB vs runtime DB on BOTH port
   and name, with a harness guard that refuses the wrong DSN before connecting) generalizes
   to "the agent's test harness may never touch real state"; router-in-lib so tests drive
   the real assembly in-process with no network; test-first AD-028 means an agent writing
   fixes authors the failing test first.
9. **Governance trail**: TRACKER.md same-commit law, per-wave outcomes/ evidence files,
   plan-review blocking bad task orderings, deferred-items ledgers — the agent's every
   action should leave the same auditable evidence chain (the mission's progress.db is
   already this pattern).
10. **Worktree-per-feature with gated branch pushes**: bf1-implementation got its own CI
    trigger so mid-wave pushes are gated ("unit-VERIFIED boundaries"), merge-to-main as the
    wave checkpoint — a sane protocol for an agent that lands fixes as PRs.
11. **Self-contained CI**: no repo/runner secrets, fixture passwords allow-listed in the
    scanner, services defined in-workflow — for an OSS Action consumed by others, this is
    the "runs on any org's runner with only BYOK env" posture.

## 7. Open questions

- Are the SUR/Harhaial line-count law (300/75 effective lines) and centralized-lints policy
  intended as gates for the storytold repos too, or per-repo?
- Should the new agent adopt the full spec-kit ladder (specify→plan→tasks→outcomes,
  plan-review gate) per feature, or the lighter audit-pool DB flow (runs/claims/events)?
- BYOK mechanics for a GitHub Action: org-level Actions secrets per provider (OpenAI/
  Anthropic/Google — the proxy already speaks all three dialects), and does the agent get
  its own escrow budget, or is spend capped purely by workflow timeouts/step limits?
- Does the maintainer want agent commits gated by the same explicit-paths-only commit
  protocol and "no commits while subagents run" rule (relevant if the Action commits)?
- The proxy's `provider_request` is forwarded verbatim (no re-encoding) — for an agent,
  prompt/response content is deliberately never metered; where does the agent's own
  transcript logging live, if anywhere?
- utoipa/OpenAPI contract-as-artifact with diff-gating: applicable if the storytold agent
  ever grows a service component, but likely overkill for an Action-only deliverable.
