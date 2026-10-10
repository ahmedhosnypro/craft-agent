# Synthesis — the new storytold agentic AI GitHub Action project

Date: 2026-10-10. Distilled from analysis/01..07 (local exploration), analysis/10..14
(websearch), 272 verified findings in progress.db, and the filmcraft PR 553 thread.
Every claim below traces to a source cited in those files.

## 1. Mission origin — the PR 553 thread

- Trigger comment (id 6096316169, ahmedhosnypro, 2026-10-10 09:54Z): land the fixes, and find
  "a more better solution, adding an experimental/beta/optional item to settings so users can
  manually enable it … for consistency in the whole storytold suite" (photocraft already has
  the behavior). Source: analysis/01.
- The maintainer's follow-up in the same thread (id 6097015929, 11:27Z) proposes the new
  project itself, in two shapes:
  (a) a parent repo plus a daily-cron GitHub Action whose AI agent (BYOK claude/openai/gemini)
      summarizes merged PRs, common patterns and duplicate code across the suite, or
  (b) a per-PR bot on all suite projects — "adding consistency and reducing future bugs".
  Source: analysis/01.
- @echelon (thanked in the comment) is a human founder (Brandon Thomas, ArtCraft), not a bot;
  ~3.4k commits across 19 suite repos in Oct 1–10. Source: analysis/01.
- The org has zero agent/bot automation of its own today (only Copilot reviews on pdfcraft and
  dependabot on artcraft). This project would be the org's first bot. Source: analysis/02.

## 2. The org the agent must serve

- 44 public repos. Active suite (pure-Rust clean-room apps): photocraft (39.4k stars, 762 open
  issues), artcraft (13.2k), lightcraft, filmcraft, pdfcraft, wordcraft, gridcraft, deckcraft,
  soundcraft, cadcraft, vectorcraft, designcraft, effectcraft, artcraftx. Shared infra:
  craft-libs, craft-fonts, craft-launcher, craft-repo-status-app, artcraft-services,
  cloud-worker, github-media. Source: analysis/02.
- CI reality: the canonical gate is fmt --check, test --workspace --locked, clippy
  --workspace --all-targets --locked -D warnings, plus xtask layers/assets/wasm/corpus/
  scorecard (photocraft/pdfcraft ci.yml; SHA-pinned actions; no macOS on PRs). But ci.yml is
  merged only in photocraft and pdfcraft — filmcraft and others have no PR test CI; their
  strict gate is a local cargo xtask ci (PR 553 admits the full gate was not run).
  Sources: analysis/02, analysis/01.
- The apps are agent-first by design: each ships a CLI MCP server, a bearer-token,
  capability-scoped TCP control channel, and stable automation ids ("built to be driven by
  AI agents, and largely built by them"); 4,984 Claude co-authored commits org-wide.
  Sources: analysis/02, analysis/03.
- Conventions to follow: craft-* naming, MIT OR Apache-2.0, the AGENTS.md + CLAUDE.md pair
  as agent contract (never-crash, everything-is-a-command, clean-room attribution, task-id
  commits, people.toml credits, measured reporting); the cross-app "craftrules" rulebook is
  private. Sources: analysis/01..03.
- The requested settings toggle is a known pattern: photocraft gates
  Interface.system_title_bar via its serde prefs schema (dialog rows auto-generate,
  applies-at-next-launch); filmcraft settings.rs is schema-driven with a `wired` flag.
  Sources: analysis/01, analysis/03.

## 3. Engine candidates (the user's agent projects)

- kimi-code (analysis/05) — Moonshot TS CLI, MIT, Node>=24 or SEA single binary. Headless
  `kimi -p --output-format stream-json` is a first-class CI mode (documented for scripts
  and CI; always-auto permissions, JSONL, disciplined exit codes, background-task policy).
  BYOK is pure env (KIMI_* model name / api key / base url / provider type) or config.toml
  providers with api_key_env indirection; permission rules + 19 shell hook events stay
  effective headless; MCP, plugins, skills, per-repo AGENTS.md discovery, subagents.
  Caveats: print mode is auto-only (self-gate any commit path), engine packages are
  private (the CLI is the stable contract), 2.x moves fast — pin the version.
- deepseek-harness / dsh (analysis/06) — DeepSeek's own MIT harness (not a kimi-code
  fork): everything-is-a-plugin on vendored Cordis; credential-name refs (apiKeyEnv)
  resolved through a credentials seam; multi-provider routes (OpenAI/Anthropic/Azure/
  Bedrock/Vertex/vLLM/custom gateways with base_url overrides); ships a github-review
  example (HMAC webhook -> ready_for_review -> read-only review Session, untrusted-JSON
  labeling, SHA pinning); keyless snapshot-replay evals; textbook key-safety CI (never
  pull_request_target, fork skips, missing-secret preflight hard-fail). Dev preview:
  adopt patterns, not packages.
- grok-build (analysis/07) — xAI pure-Rust agent (~100 crates, Apache-2.0, no external
  contributions): ApiBackend over three protocols (OpenAI chat_completions, OpenAI
  responses, Anthropic messages); BYOK TOML [model_providers.*] with base_url / env_key /
  extra_headers / command-based auth_provider; headless `grok -p` JSON output with
  --tools allowlist, --max-turns, --allow/--deny globs (docs include CI review recipes);
  Landlock/Seatbelt sandbox; no GitHub API layer — an Action must add its own.
- Verdict: fastest path = drive the MIT kimi-code CLI headless inside the Action; a
  Rust-native self-owned engine should lift grok-build's patterns (Tool trait, sampler,
  BYOK schema, permission layering); dsh contributes harness/plugin and eval patterns.
  The vendor actions (claude-code-action, codex-action) are the UX baseline to match.

## 4. Ecosystem and competition (websearch-verified 2026-10-10)

- Configurable agentic GitHub Actions that exist today, in answer to "can any agentic
  action be configured?":
  - anthropics/claude-code-action (MIT, 9,463 stars, very active): runs entirely on your
    runner; triggers = @claude mention / assignment / label / automation prompt; six
    model-auth paths (ANTHROPIC_API_KEY, OAuth subscription token, OIDC workload-identity
    federation, Bedrock/Vertex/Foundry); Claude GitHub App with short-lived repo-scoped
    tokens; deliberate limits: no PR approval, no formal review, no merge/rebase, no auto
    PR creation, bash off by default, single sticky comment, base-branch restore of agent
    config on PRs (anti-injection). Source: analysis/11.
  - openai/codex-action (Apache-2.0, 1,262 stars, pushed 2026-10-09): Codex CLI + a
    Responses-API proxy on your runners; OPENAI_API_KEY / Azure; permission profiles
    (read-only / workspace), drop-sudo safety strategy, allow-users/allow-bots gating;
    official two-job auto-fix pattern (agent job contents:read -> patch artifact ->
    privileged PR job without the key). Sources: analysis/12, analysis/13.
  - GitHub native Agentic Workflows (gh-aw, public preview, MIT): markdown compiled to
    hardened .lock.yml workflows; BYOK engines copilot/claude/codex/gemini; safe-outputs
    write gating; max-ai-credit caps. Sources: analysis/12, analysis/14.
  - Copilot cloud agent (platform-native; Actions minutes + AI credits; ARC-only
    self-hosting for review) and the Codex Connector GitHub App (ChatGPT-plan-linked;
    GitHub features unavailable on the API-key tier). Sources: analysis/12.
- SaaS-trigger-only actions (no BYOK; execution on vendor cloud): google-labs-code/
  jules-action (254 stars, ~10 months quiet), Factory-AI/droid-action (56 stars).
  Source: analysis/10.
- Open-source frameworks without a first-class Action: deepseek-harness (246,813 stars
  since Aug 2026; GitHub glue is a 22-star community plugin), OpenHands (90,477; old
  resolver action archived), pr-agent (13,332; review-only, BYOK), SWE-agent (20,516),
  goose (55,129, Rust, no Action), aider (49,456, quiet ~4.5 months). Source: analysis/10.
- Dead/pivoted: sweep (dormant at JetBrains), Mentat (archived), aider-sweeper (deleted),
  openhands-resolver (archived), sourcegraph cody (repo deleted). Source: analysis/10.
- The confirmed gap: no provider-neutral BYOK action; nothing Rust-native ships as an
  Action; nobody closes the edit -> cargo gates -> iterate -> status-check loop;
  everything is single-repo (our ~22-repo suite shape is unserved); security is opt-in
  everywhere. Source: analysis/10.

## 5. Building blocks and security baseline

- Identity: actions/create-github-app-token v3.2.0 (client-id input; 1h token; git
  identity <app-slug>[bot]) — GitHub recommends App tokens over PATs; the built-in
  GITHUB_TOKEN never re-triggers workflows except workflow_dispatch / repository_dispatch
  (free anti-recursion). Source: analysis/14.
- Trigger semantics: issue_comment fires on both issue and PR comments (the @mention
  entrypoint; write-access non-bot actors only in interactive mode); pull_request/issues
  `labeled` activity types; workflow_run grants secrets+write even when the triggering run
  had none (enables read-only-analysis -> privileged-fix job splits); repository_dispatch
  for external triggers; fork PRs deliver events but with read-only token and no secrets.
  Source: analysis/14.
- Rust toolchain on runners: dtolnay/rust-toolchain (cachekey output) then
  Swatinem/rust-cache (must follow the toolchain step); actions/checkout with
  fetch-depth: 0 for full-history analysis. Source: analysis/14.
- Runner economics: ubuntu-latest = 4 vCPU / 16 GB / 14 GB disk; ubuntu-slim = 1-cpu
  15-minute container runner (cheap triage); larger runners and ARC
  (actions-runner-controller, actively maintained) for heavier agent work.
  Source: analysis/14.
- MCP-in-CI building block: github/github-mcp-server (33,488 stars). Source: analysis/14.
- Packaging: both vendor references are composite actions wrapping a runtime-installed
  CLI with SHA-pinned third-party steps (claude-code-action: composite+Bun;
  codex-action: composite+Node24). For us: a composite action that downloads our
  self-contained Rust binary — Docker-like reproducibility with cross-OS reach.
  Release convention: own repo, semver tags, moving v1 major tag, full-SHA pinning.
  Source: analysis/14.
- Security checklist (21 doc-cited items in analysis/13): fork PRs are secretless and
  read-only by design; never pull_request_target (GitHub blocks it by default in public
  repos from 2026-11-02; actions/checkout now blocks unsafe fork checkouts); secrets via
  env-var interpolation only; environment protection rules as the human-approval gate;
  ephemeral GitHub-hosted runners for public repos (self-hosted almost never); workflow
  timeout/concurrency caps; secret scanning already covers OpenAI/Anthropic key patterns.
- Architecture baseline (analysis/13): split identity — read-token analysis job + trusted
  publisher job with the App token; model key isolated from job-level env; degraded
  read-only fork mode; api_key/base_url/model inputs for provider neutrality;
  label/comment-derived state for resumability.

## 6. Reusable patterns from the user's own projects

- harhaial-backend (analysis/04): one 12-stage gate script (secrets scan, fmt,
  line-count, clippy -D warnings, migration apply-verify, nextest, OpenAPI contract,
  cargo deny, machete, locked build) run identically by agent, pre-commit and CI —
  "task done when gate exits 0". Its security-audit-pool skill is the closest existing
  analog to this project: a parallel subagent pool supervised by an in-project SQLite DB
  (runs/claims/events, watchdogs, UNIQUE-claim advisory locks, no git writes in workers,
  verify-before-flag with file:line + attacker story, self-learning experience index).
  Plus escrow Reserve-and-Settle metering (hold credits, settle exactly once) for spend
  caps; advisory postToolUse write-time hooks vs enforcement in the gate; self-contained
  CI with zero repo secrets.
- deepseek-harness (analysis/06): store credential NAMES in config (apiKeyEnv), resolve
  per-request through a credentials seam; append-only session log as the single source
  of model context; keyless recorded-session replay as the eval method; the HMAC-webhook
  -> read-only-review-session flow as a safe first agent surface.
- grok-build (analysis/07): the BYOK TOML schema (base_url, env_key, extra_headers,
  auth_provider commands) and credential priority order; the three-protocol ApiBackend;
  the unified Tool trait with schemars-derived schemas and typed resources; headless
  JSON output contract; permission-mode + glob allow/deny + Landlock layering.
- kimi-code (analysis/05): the print-mode contract (stream-json JSONL, disciplined exit
  codes, background-task policy); permission rules that stay effective headless;
  AGENTS.md / CLAUDE.md / skills discovery matching what the suite already ships.
- filmcraft/photocraft (analysis/01, 03): automation ids + MCP/control protocol mean the
  agent can eventually drive the real apps headlessly (offscreen snapshot rendering) for
  visual verification of UI changes — a capability no competitor has.

## 7. Recommended architecture for the new project

- Shape: new repo under storytold using the craft-* convention (suggested: craft-agent),
  dual MIT OR Apache-2.0 license, AGENTS.md + CLAUDE.md pair, install-quickstart README.
  Deliverable = a composite GitHub Action that downloads a self-contained Rust binary
  (analysis/14 packaging recommendation) plus a GitHub App for the bot identity.
- Two jobs, split identity (analysis/13 + codex-action precedent):
  1. Analysis job: read-only GITHUB_TOKEN (or App token with contents:read), BYOK model
     key, runs the agent, posts a sticky comment; safe on any event, including fork PRs
     in a degraded read-only mode.
  2. Publisher job (opt-in): gated by a GitHub environment with required reviewers;
     App token with contents:write pushes a branch. The agent never approves, merges
     or force-pushes (match claude-code-action's deliberate limits) — humans merge.
- BYOK, provider-neutral (analysis/13, 07, 06): inputs api_key_env / base_url / model;
  OpenAI-compatible endpoints + Anthropic Messages + gateways supported day one;
  credential names in config, resolved at runtime; escrow-style spend metering with a
  max-budget stop (analysis/04) and gh-aw-style credit caps.
- Engine strategy: phase 1 drives the MIT kimi-code CLI headless (kimi -p, stream-json)
  — fastest to value, matches the suite's AGENTS.md conventions; phase 2 evaluates a
  Rust-native engine inside the binary lifting grok-build patterns, behind an adapter
  seam so the maintainer's BYOK claude/openai/gemini ask is satisfied either way.
- Modes:
  (a) Interactive: @craft-agent mention on issues/PRs (issue_comment trigger), label
      assignment, PR-review assistance with cross-platform blast-radius analysis like
      the echelon review in PR 553 (analysis/01).
  (b) Scheduled digest (the maintainer's option (a)): daily cron in an ops/parent repo
      summarizing merged PRs, common patterns, duplicate code across the suite.
  (c) Consistency missions: cross-repo parity checks (e.g. the PR 553 settings toggle:
      photocraft precedent vs filmcraft schema-driven settings) raised as issues/PRs.
- Verification loop — the differentiator: before proposing changes, run the repo's own
  gates (fmt --check, test --workspace --locked, clippy -D warnings, xtask targets) via
  dtolnay/rust-toolchain + Swatinem/rust-cache; iterate until green or report honest
  failure (the suite demands measured, not estimated, reporting — analysis/01). For
  repos without merged ci.yml, the agent effectively brings the gate to the PR.
- State & observability: SQLite run/claim/event tracking (the proven audit-pool pattern,
  analysis/04) for resumability, dedup, watchdogs and cost accounting.
- Safety: prompt-injection defenses (sanitize untrusted issue/PR text, restore
  AGENTS.md from the base branch on PRs), timeouts, concurrency groups, ubuntu-slim for
  cheap triage, full-SHA pinning, Co-Authored-By trailers for agent commits.

## 8. Rollout plan

- Phase 0 (setup): create storytold/craft-agent; register the GitHub App; add BYOK
  secrets as org-level Actions secrets or environments (no CI BYOK precedent exists
  yet — analysis/02); decide craftrules (private) access for the agent.
- Phase 1 (pilot): interactive reviewer on photocraft (merged ci.yml, 39.4k stars) and
  filmcraft (no PR CI — immediate verification value), comment-only; measure with the
  dsh keyless-replay eval method (analysis/06).
- Phase 2 (digest): daily cron in the ops repo — merged-PR summaries, common patterns,
  duplicate-code candidates across the suite (the maintainer's stated option (a)).
- Phase 3 (org-wide): reusable workflow or a new .github org repo (the slot is open —
  analysis/02); high-traffic repos first (photocraft, artcraft, lightcraft, filmcraft,
  pdfcraft), then green-field apps (wordcraft, gridcraft, deckcraft).

## 9. Open questions for the maintainer

1. Engine: wrap the kimi-code CLI now, build the Rust-native engine first, or both
   behind an adapter?
2. Scope: comment/review-only, or also push fix branches (human-gated)?
3. Identity: own GitHub App (e.g. craft-agent[bot]); keep Copilot reviewing pdfcraft
   in parallel?
4. BYOK placement: org Actions secrets vs per-repo environments; which providers first
   (claude/openai/gemini were named in the thread; kimi/deepseek/grok engines exist
   in-house)?
5. Access to the private craftrules rulebook; do macOS gates matter (the org keeps its
   5 macOS runners for release — analysis/02)?
6. Repo name: craft-agent vs agentcraft vs other.

## 10. Source index

- analysis/01 filmcraft PR 553 thread, @echelon identity, filmcraft gates/agent rules
- analysis/02 storytold org map: repos, CI gates, bots, shared infra
- analysis/03 photocraft: CI, agent contract, control protocol, settings mechanism
- analysis/04 harhaial-backend bf1: gate, audit-pool orchestration, escrow metering
- analysis/05 kimi-code: headless mode, BYOK, permissions — engine candidate
- analysis/06 deepseek-harness: plugin harness, credential seam, github-review example
- analysis/07 grok-build: Rust agent patterns, BYOK TOML, sandboxing
- analysis/10 public agentic action landscape and gap analysis
- analysis/11 claude-code-action deep dive (auth, triggers, limits, BYOK)
- analysis/12 OpenAI Codex GitHub paths, codex exec headless, Copilot cloud agent, gh-aw
- analysis/13 BYOK action examples + 21-item security checklist
- analysis/14 verified Action building blocks, runners, packaging, skeleton pipeline
- progress.db: 272 findings rows + events log (WAL SQLite at the project root)
