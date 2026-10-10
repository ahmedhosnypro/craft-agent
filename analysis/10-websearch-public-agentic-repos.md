# Public agentic AI GitHub Action solutions — landscape scan

- Slug: `ws1-public-agentic-repos` · Date: 2026-10-10
- Question: which public projects already deliver an agentic AI coding agent integrated with GitHub — especially open-source, self-hostable, real GitHub Actions running in your CI with BYOK (bring-your-own-key)?

## 1. Methodology

Web searches (Google-backed `agy_web_search`; 9 queries):
1. "open source AI coding agent as GitHub Action bring your own API key 2026 self-hosted"
2. "Google Jules AI coding agent GitHub integration how it works 2026"
3. "Factory droid AI coding agent GitHub integration 2026 droid CLI GitHub App"
4. "Cursor background agents GitHub integration 2026 how it works pull requests"
5. "GitHub Copilot coding agent assign issue pull request premium requests 2026"
6. "Mentat AI coding assistant AbanteAI discontinued OR deleted 2024 2025"
7. "aider sweeper paul-gauthier GitHub action issue fixing still exists"
8. "Sourcegraph Cody Amp 2026 GitHub pull requests integration status"
9. "CodeRabbit Greptile Devin AI pull request review bots 2026 GitHub App self-hosted"

GitHub API searches (`gh api search/repositories`, ~14 queries): candidate lookups for
sweep · aider-sweeper · mentat · cody · openhands-resolver · codex github action ·
deep-agent-action · goose action · SWE-agent action · aider github-action · claude code
action alternatives · deepseek harness + dsh agent · pr-agent + codez ·
topic:ai-agent+topic:github-actions · "agentic github action" · "github action" +
"bring your own" agent.

Verification: every catalog entry was checked against the GitHub repo API this session
(stars, pushed_at, archived, license); key READMEs and docs pages were fetched raw.
Facts resting only on search-result synthesis (no primary page fetched) are marked
"(search synthesis)". One agy token expired mid-session (HTTP 401) and was re-minted
via `agy -p`; searches resumed with no data loss. Budget used: 9 web searches,
~14 GitHub searches, ~12 page/repo fetches.

## 2. Catalog of comparable public solutions

Legend: **Action** = real GitHub Action — agent runs in YOUR CI (incl. self-hosted
runners). **App/SaaS** = hosted product on vendor infrastructure. **CLI** = local
terminal agent, no first-class GitHub event integration. **BYOK** = you supply your
own model API key.

### A. First-party vendor Actions — true Actions in your CI, BYOK

**A1. anthropics/claude-code-action — the category reference implementation**
- URL: https://github.com/anthropics/claude-code-action
- What: official Anthropic action wrapping Claude Code for GitHub PRs and issues —
  answers questions, reviews code, implements fixes and features. TypeScript.
- Integration: Action + GitHub App hybrid. A GitHub App (created via /install-github-app
  or manual setup) listens and authenticates; README states the action "executes
  entirely on your own GitHub runner."
- Trigger events: @claude mentions in issues/PRs/comments, issue assignment, or explicit
  prompt inputs in automation workflows; execution mode auto-selected from workflow context.
- Self-hosted runners: yes. BYOK: yes — ANTHROPIC_API_KEY secret or workload identity
  federation; also Amazon Bedrock, Google Vertex AI, Microsoft Foundry.
- License MIT · 9,463 stars · 2,196 forks · pushed 2026-10-09 (created 2025-05-19).
- Sources: repo API + README fetched 2026-10-10.

**A2. anthropics/claude-code-base-action — the trustless building block**
- URL: https://github.com/anthropics/claude-code-base-action
- What: mirror of the base-action directory of claude-code-action; thin wrapper that
  installs and runs Claude Code as a workflow step for custom automations.
- Integration: pure Action step, no event listening. README warns it enforces no trust
  boundaries of its own — for untrusted input (issues, fork PRs) use A1, which adds
  actor permission checks and restores project config from the base ref.
- Self-hosted runners: yes. BYOK: same Anthropic credentials as A1.
- License MIT · 999 stars · pushed 2026-10-09.
- Sources: repo search API + README fetched 2026-10-10.

**A3. openai/codex-action — official OpenAI building block**
- URL: https://github.com/openai/codex-action
- What: installs the Codex CLI and wires a secure proxy to the Responses API; README
  frames it as "create your own pull request bot" (example: review on PR opened).
- Integration: pure Action step — you define the trigger; ships security docs
  (docs/security.md) and a permissions model for filesystem/network access.
- Trigger events: any workflow event you wire up. Self-hosted runners: yes.
- BYOK: yes — OPENAI_API_KEY or AZURE_OPENAI_API_KEY as Actions secrets.
- License Apache-2.0 · 1,262 stars · pushed 2026-10-09.
- Related: openai/codex CLI itself (128,499 stars, Apache-2.0, pushed 2026-10-10) also
  offers a hosted "Codex cloud" code-review SaaS — both halves exist.
- Sources: repo API + README fetched 2026-10-10.

### B. Vendor Actions that trigger hosted SaaS (the Action is a remote control)

**B1. google-labs-code/jules-action**
- URL: https://github.com/google-labs-code/jules-action
- What: "Add a powerful cloud coding agent to your GitHub workflows" — triggers Jules.
- Integration: Action invokes the Google-hosted Jules agent; execution happens on a
  Google-managed cloud VM, not your runner. Jules: connect GitHub at jules.google,
  trigger from issues via a label, review the plan, get a PR; reads AGENTS.md
  (search synthesis citing jules.google/docs).
- Trigger events: any Actions event, label, or schedule you wire to it.
- Self-hosted runners: no. BYOK: no (Gemini served by Google).
- License MIT (action glue only) · 254 stars · pushed 2025-12-15 (~10 months quiet).
- Sources: repo API 2026-10-10 + search synthesis for Jules behavior.

**B2. Factory-AI/droid-action**
- URL: https://github.com/Factory-AI/droid-action
- What: "Factory's Droid actions" — runs Droid in workflows; Droid CLI ships
  /install-github-app and /install-code-review installers; @droid mentions on
  issues/PRs; headless droid exec mode for CI (search synthesis citing
  docs.factory.com and the GitHub Marketplace listing).
- Trigger events: @droid mentions and PR events per installed workflow.
- Self-hosted runners: CLI runs in your CI, but models/billing flow through Factory.
- BYOK: no — Factory account required. License: none (proprietary) · 56 stars ·
  pushed 2026-10-08.
- Sources: repo API 2026-10-10 + search synthesis for Droid behavior.

### C. Open-source agent frameworks with GitHub integration (self-hostable)

**C1. deepseek-ai/deepseek-harness (DSH) — the 2026 breakout**
- URL: https://github.com/deepseek-ai/deepseek-harness
- What: "DeepSeek Harness: Everything is a Plugin." Open-source agent harness
  (TypeScript) with a fast-growing plugin ecosystem: awesome-dsh-plugin 18,291 stars,
  desktop distros 12,908 + 3,155 stars.
- Integration: local harness by default; GitHub glue arrives via young plugins:
  - PerryLink/dsh-github — composite action, PR review bot with idempotent inline
    comments, status-check gate, every write gated by human approval
    (Apache-2.0, 22 stars).
  - Lixiaoyiao/deepseek-harness-action — AI code review, CI diagnosis, auto fix,
    issue-to-PR (MIT, 22 stars).
- Trigger events: whatever the plugin wires (PR review; issue-to-PR).
- Self-hosted runners: yes. BYOK: yes (model providers incl. DeepSeek).
- License MIT · 246,813 stars · created 2026-08-13 · pushed 2026-10-09.
- Sources: repo APIs 2026-10-10.

**C2. OpenHands/OpenHands (org renamed from All-Hands-AI) — pivoted platform**
- URL: https://github.com/OpenHands/OpenHands
- What: README now bills it as "the self-hosted developer control center for coding
  agents and automations" (Agent Canvas, beta): runs OpenHands, Claude Code, Codex,
  Gemini, Pi, OpenCode, or any ACP-compatible agent across local, Docker, VM, or
  cloud backends; automations react to GitHub/Slack/Linear webhook events; optional
  OpenHands Cloud / Enterprise.
- Integration: self-hosted control center + webhook automations — NOT an Action. The
  old GitHub-native issue-resolver (All-Hands-AI/openhands-resolver, issue-to-PR) was
  archived 2024-11-18 (130 stars); no official replacement Action found.
- Trigger events: webhook-based automations (incl. GitHub) and schedules.
- Self-hosted: yes (Docker/VM). BYOK: yes (per-agent-backend model keys).
- License MIT · 90,477 stars · pushed 2026-10-10.
- Sources: repo API (All-Hands-AI path redirects) + README fetched 2026-10-10.

**C3. The-PR-Agent/pr-agent — review-focused, Action-native, BYOK**
- URL: https://github.com/The-PR-Agent/pr-agent
- What: open-source PR review/describe/improve bot. Repo moved out of QodoAI
  (QodoAI/pr-agent is 404); description: "The Original Open-Source PR Reviewer. This
  project is not the Qodo free tier." Docker images moved codiumai/ to pragent/
  (releases 0.34.2+).
- Integration: README lists "GitHub Action (Recommended)" plus CLI, Docker,
  self-hosted, webhooks — a true Action; self-hosting with your OpenAI key is the
  documented BYOK path.
- Trigger events: PR events per workflow wiring (opened/synchronize).
- Self-hosted runners: yes. BYOK: yes (OpenAI et al.).
- License MIT · 13,332 stars · pushed 2026-10-10.
- Scope limit: review-centric; no autonomous issue-to-implement-to-PR loop.
- Sources: repo API + README fetched 2026-10-10.

**C4. SWE-agent/SWE-agent (moved from princeton-nlp) — research agent**
- URL: https://github.com/SWE-agent/SWE-agent
- What: "takes a GitHub issue and tries to automatically fix it, using your LM of
  choice" (NeurIPS 2024); dev effort now on mini-swe-agent (~100 lines of Python,
  65% SWE-bench Verified).
- Integration: local CLI / Codespaces; NO official GitHub Action.
- Self-hosted: yes. BYOK: yes (LM of your choice).
- License MIT · 20,516 stars · pushed 2026-10-06.
- Sources: repo API + README fetched 2026-10-10.

**C5. aaif-goose/goose (moved from block/goose)**
- URL: https://github.com/aaif-goose/goose
- What: open-source extensible AI agent (CLI/desktop) — "install, execute, edit, and
  test with any LLM"; supports custom distros with preconfigured providers. Written
  in Rust. block/goose redirects to the aaif-goose org.
- Integration: local agent; no official GitHub Action found (README/org scan).
- Self-hosted: yes (local). BYOK: yes (any LLM).
- License Apache-2.0 · 55,129 stars · pushed 2026-10-09.
- Sources: repo API + README fetched 2026-10-10.

**C6. gptme/gptme**
- URL: https://github.com/gptme/gptme
- What: terminal agent with local tools (writes code, uses terminal, browses the
  web); persistent-agent workspace template. Org listing shows no GitHub Action repo.
- Integration: CLI only. Self-hosted: yes. BYOK: yes.
- License MIT · 4,446 stars · pushed 2026-10-10.
- Sources: repo API + org listing fetched 2026-10-10.

**C7. Aider-AI/aider**
- URL: https://github.com/Aider-AI/aider
- What: AI pair programming in your terminal (git-native multi-file edits).
- Integration: CLI only; no official GitHub Action. The old paul-gauthier/aider-sweeper
  demo (aider auto-fixing GitHub issues) is deleted (404 this session).
- Self-hosted: yes. BYOK: yes. License Apache-2.0 · 49,456 stars · pushed 2026-05-22
  (~4.5 months quiet at scan time; community discusses maintainer availability).
- Sources: repo API 2026-10-10 + search synthesis.

**C8. aeonfun/aeon**
- URL: https://github.com/aeonfun/aeon
- What: "The most autonomous AI agent framework: runs unattended on GitHub Actions,
  self-healing skills, drives Claude Code, Grok, Codex & more. No approval loops."
- Integration: Actions-native autonomous framework — agents live inside CI, driven by
  cron/events, configured via aeon.yml. No root README (AGENTS.md/CLAUDE.md/STRATEGY.md).
- Self-hosted runners: yes. BYOK: yes (drives CLI agents with your keys).
- License MIT · 770 stars · pushed 2026-10-10.
- Sources: repo API + repo contents listing 2026-10-10.

### D. Hosted GitHub-App products (for contrast — none run in your CI)

**D1. GitHub Copilot coding agent (cloud agent)**
- What: assign an issue to Copilot (or request Copilot code review on a PR); it
  researches the repo, plans, implements, tests in a GitHub-managed ephemeral sandbox,
  and opens a draft PR (search synthesis citing docs.github.com).
- Billing: the fetched docs page marks "premium requests" as LEGACY — only for annual
  Pro/Pro+ users who remained on request-based billing after June 1, 2026; Spark and
  the Copilot cloud agent moved to dedicated SKUs on Nov 1, 2025; Copilot code review
  has a model multiplier of 13 from June 1, 2026.
- BYOK: no. Self-hosted: no. Sources: docs.github.com fetched 2026-10-10.

**D2. Google Jules** — connect GitHub at jules.google; cloud-VM agent; issue-label
trigger; plan-review step; PR output; AGENTS.md support. No BYOK, no self-host.
(search synthesis citing jules.google)

**D3. Factory Droid** — @droid mentions on issues/PRs; GitHub App + droid-action;
headless droid exec for CI; Factory account required; mixes frontier models.
Proprietary. (search synthesis citing docs.factory.com + marketplace)

**D4. Cursor background/cloud agents** — isolated cloud VMs, parallel agents, branch +
PR creation, results reviewed back in the IDE; SaaS subscription; not a CI Action.
(search synthesis; primary pages not fetched)

**D5. Devin (Cognition)** — autonomous software engineer: cloud or on-machine, own
Windows/Mac/Linux VMs, browser testing, parallel cloud agents, @Devin in Slack/Teams,
watches issues and failed CI, automations and security swarms, Devin Review.
SaaS/enterprise, no OSS. Source: cognition.ai/devin fetched 2026-10-10.

**D6. CodeRabbit** — hosted "governance & control for agentic development": AI review,
PR triage/scoring, change-stack explainability for huge agent diffs, security
monitoring. GitHub App SaaS. Source: coderabbit.ai fetched 2026-10-10.

**D7. Greptile** — hosted AI code review: plain-English custom rules, learns from your
review comments, Trex sandbox that runs PR branches to catch runtime bugs; "22,000+
teams" claim. GitHub App SaaS. Source: greptile.com fetched 2026-10-10.

### E. Dead or pivoted (verified gone from the GitHub-bot space)

- **sweepai/sweep** (7,708 stars): now described as "Sweep: AI coding assistant for
  JetBrains"; no push since 2025-09-18; license NOASSERTION — the issue-to-PR bot
  era of Sweep is over.
- **Mentat**: AbanteAI/mentat deleted (404); archived as AbanteAI/archive-old-cli-mentat
  (Apache-2.0, 2,539 stars, archived, last push 2025-01-07).
- **Sourcegraph Cody**: sourcegraph/cody repo deleted (404); sourcegraph-public-snapshot
  archived (2024). Cody = enterprise IDE assistant; Amp = terminal agent; no GitHub
  PR bot — Sourcegraph exited this space. (repo APIs + search synthesis)

### F. Community long-tail Actions (real Actions, small adoption)

- mirrajabi/aider-github-action — runs Aider inside workflows; 59 stars; pushed
  2026-02-16 (plus mirrajabi/aider-github-workflows, 37 stars).
- cholf5/claude-code-in-action — Claude Code in Actions; 340 stars; pushed 2026-01-05.
- potproject/code-agent — invoke coding-agent CLIs from issue/PR comments; 37 stars;
  pushed 2025-04-30.
- icoretech/codex-action — run Codex CLI non-interactively; 13 stars; pushed 2026-10-10.
- gersmann/codex-review-action — Codex PR review; 6 stars; pushed 2026-09-16.
- Adjacent tools (review/QA, not full agents): coldteadotai/pr-lens (1,918 stars — PR
  walkthrough as App/Action/CLI), scanaislop/aislop (681 — deterministic post-agent
  cleanup, MIT, no LLM at runtime), promptfoo/promptfoo-action (75 — LLM evals).
- Dozens of 0-3 star DIY review actions surfaced in search; no breakout hit.
- Sources: gh api search results 2026-10-10.

## 3. Gaps and opportunities

1. **Vendor lock-in defines the only first-class Actions.** The two dominant BYOK
   Actions (claude-code-action, codex-action) are maintained by model vendors and
   route to their own model families (Anthropic direct/Bedrock/Vertex/Foundry;
   OpenAI/Azure). No first-class open-source Action is provider-neutral
   (OpenAI + Anthropic + DeepSeek + OpenRouter + local Ollama/vLLM from one config).
2. **No Rust-native Action.** Every verified solution is TypeScript or Python
   (claude/codex actions: TS; DSH: TS; OpenHands/SWE-agent/aider/gptme: Python).
   goose is Rust but CLI-only, with no GitHub Action. Nothing ships a Rust agent
   binary as a GitHub Action.
3. **GitHub App ceremony vs DIY-bot trade-off.** claude-code-action needs a GitHub
   App (admin rights, app secrets, setup ceremony); codex-action avoids the App but
   leaves you to build the bot yourself (PR context, diff fetching, comment loop).
   Nobody ships an opinionated zero-ceremony mode (GITHUB_TOKEN-only) with an
   optional App upgrade path.
4. **Verification-first loops are thin.** Most Actions post text; few close the loop
   edit → run cargo test/clippy/rustfmt → iterate → status check. dsh-github gates
   writes on human approval + status checks, but it is a 22-star plugin; nothing is
   Rust-toolchain-aware (workspaces, crate scoping).
5. **Multi-repo/org-suite awareness: absent.** Every solution is single-repo. For a
   ~22-app pure-Rust suite, cross-repo triage, shared-crate bumps and coordinated
   rollouts are unserved.
6. **Security posture is opt-in.** claude-code-base-action's README admits it enforces
   no trust boundaries of its own; untrusted-input handling is delegated upward to the
   app layer. An injection-resistant, egress-denying, least-privilege-by-default
   sandbox is a differentiator no incumbent markets.
7. **Harness momentum vs integration maturity.** DSH (246,813 stars in ~2 months)
   proves demand for plugin harnesses, but its GitHub glue is 22-star community
   plugins — the "harness + battle-tested GitHub Action" combination remains open.
8. **The 2023-24 pioneer wave is dead.** Sweep (pivot to JetBrains), Mentat (archived),
   aider-sweeper (deleted), openhands-resolver (archived), Sourcegraph (exited bots) —
   survivors are vendor Actions or heavy self-hosted platforms. The independent middle
   is empty.

## 4. Implications for our project

Positioning gap confirmed: as of 2026-10-10 there is no prominent open-source,
provider-neutral, BYOK, Rust-native agentic GitHub Action that runs on your own
runners. Anthropic and OpenAI each occupy their walled half; everything else is a
hosted app or a heavyweight platform.

1. **Ship a true Action, not a platform.** Market structure: heavy self-hosted control
   centers (OpenHands 90k, DSH 247k), vendor Actions, hosted apps. The empty middle is
   a single installable Action (composite step wrapping a Rust binary) — zero infra,
   zero server, BYOK.
2. **Provider-neutral BYOK is the wedge.** One config for OpenAI-compatible +
   Anthropic + OpenRouter + local endpoints. claude-code-action locks to Anthropic
   model families; codex-action to OpenAI/Azure; pr-agent is review-only; DSH GitHub
   plugins are nascent.
3. **Rust-first verification loop as signature behavior.** Native cargo fmt --check /
   clippy -D warnings / cargo test workspace semantics: fix → verify → iterate → only
   comment or push when green, with check-run status and structured summary. No
   existing Action does this out of the box.
4. **Serve the all-Rust suite shape.** Add suite-level behaviors single-repo bots
   cannot: org-wide triage, workspace-aware scoping, dependency-bump PRs across ~22
   repos.
5. **Security as a headline.** Defaults: egress allow-list (model endpoint only),
   sandboxed handling of untrusted issue/PR text, least-privilege GITHUB_TOKEN first
   with optional GitHub App. base-action's trust-model warning shows the incumbent gap.
6. **Copy the best UX, stay neutral.** Adopt claude-code-action's interaction patterns
   (mention triggers, mode detection, progress tracking, structured outputs) and
   codex-action's building-block posture — but model-agnostic.
7. **License MIT or Apache-2.0** (matches every surviving OSS incumbent); distribute
   the agent as a Rust binary via the Action — fast install, small runner footprint,
   consistent with the storytold suite (filmcraft, photocraft et al.).
8. **Move before the window closes.** DSH's plugin gravity could spawn a first-party
   GitHub plugin; Copilot/Jules/Droid free tiers keep pressuring DIY Actions on
   convenience. The defensible spot is data control + cost predictability +
   Rust-native verification, shipped soon.
