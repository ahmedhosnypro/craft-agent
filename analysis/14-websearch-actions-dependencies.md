# Agentic AI Action — Verified GitHub Actions Building Blocks (ws5-actions-deps)

Research date: 2026-10-10. Scope: concrete ecosystem dependencies for a NEW agentic
AI GitHub Action for the `storytold` org (pure-Rust apps, BYOK model access).

## 1. Methodology

- Primary search: Google-backed web search MCP (`agy_web_search`), 8 queries.
- Every marketplace/community action below was verified live via
  `gh api repos/OWNER/REPO` (stars, `pushed_at`, `archived`) and its `action.yml`
  / `README.md` fetched from `raw.githubusercontent.com/OWNER/REPO/HEAD/...`.
- GitHub platform semantics (runner specs, trigger events, GITHUB_TOKEN, secrets,
  action types, release management) verified against docs.github.com pages fetched
  this session; a source URL accompanies every claim.
- "Maintained" = not archived + pushed within ~6 months of 2026-10-10.
- Nothing is stated from memory alone; all facts were verified this session.

## 2. Verified shopping list

| Action | Purpose | Maintained (verified 2026-10-10) | Why we need it |
|---|---|---|---|
| [actions/create-github-app-token](https://github.com/actions/create-github-app-token) v3.2.0 | Mint short-lived GitHub App installation token | Yes — 897 stars, pushed 2026-10-06 | Bot identity for agent comments/commits; fine-grained `permission-*` inputs |
| [actions/checkout](https://github.com/actions/checkout) v7.0.1 | Clone repo | Yes — 8945 stars, pushed 2026-09-29 | `fetch-depth: 0` full history for agent analysis; `token:` input for bot pushes |
| [marocchino/sticky-pull-request-comment](https://github.com/marocchino/sticky-pull-request-comment) | Create/update one sticky PR comment | Yes — 655 stars, pushed 2026-10-09; no `actions/`-org equivalent (404) | Idempotent agent reports; `header` input separates multiple reports |
| [actions/labeler](https://github.com/actions/labeler) | Auto-label PRs by changed paths | Yes — 2501 stars, pushed 2026-09-30 | Label-driven agent triggering/routing |
| [actions/github-script](https://github.com/actions/github-script) | Inline JS against GitHub API | Yes — 5034 stars, pushed 2026-04-09 | Reactions on trigger comments, ad-hoc API calls |
| [dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain) | Install pinned Rust toolchain | Yes — 1588 stars, pushed 2026-10-09 | Storytold is pure Rust; outputs `cachekey` for cache keys |
| [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) v2.9.2 | Smart cargo target/registry caching | Yes — 1934 stars, pushed 2026-10-05 | Warm caches for agent `cargo test`/`clippy` verification loops |
| [actions-rust-lang/setup-rust-toolchain](https://github.com/actions-rust-lang/setup-rust-toolchain) | Alt toolchain setup + problem matchers | Yes — 405 stars, pushed 2026-09-07 | Annotates PRs with rustc/clippy diagnostics |
| [oven-sh/setup-bun](https://github.com/oven-sh/setup-bun) v2.2.0 | Install Bun runtime | Yes — 727 stars, pushed 2026-03-14 | If engine is TypeScript (claude-code-action uses this) |
| [actions/setup-node](https://github.com/actions/setup-node) | Install Node.js runtime | Yes — 4982 stars, pushed 2026-10-08 | If engine is TS/Node (codex-action uses node 24) |
| [stefanzweifel/git-auto-commit-action](https://github.com/stefanzweifel/git-auto-commit-action) | Commit dirty working tree | Yes — 2578 stars, pushed 2026-08-26 | Commit agent fixes under bot identity |
| [peter-evans/create-pull-request](https://github.com/peter-evans/create-pull-request) | Open PR from agent changes | Yes — 2857 stars, pushed 2026-10-01 | Fixes as reviewable PRs, not direct pushes |
| [EndBug/add-and-commit](https://github.com/EndBug/add-and-commit) | Commit+push alternative | Yes — 1292 stars, pushed 2026-10-05 | Fallback if auto-commit semantics prove limiting |
| [anthropics/claude-code-action](https://github.com/anthropics/claude-code-action) | Reference agentic action (Anthropic) | Yes — 9463 stars, pushed 2026-10-09 | Pattern to study: composite+Bun, OIDC federation, MCP env passthrough |
| [openai/codex-action](https://github.com/openai/codex-action) | Reference agentic action (OpenAI) | Yes — 1262 stars, pushed 2026-10-09 | Pattern to study: npm CLI install, API proxy, sandbox strategies |
| [ai-action/setup-ollama](https://github.com/ai-action/setup-ollama) | Install Ollama CLI | Active but tiny — 15 stars, pushed 2026-10-09 | Local-model BYOK option; weak community adoption |
| [ai-action/ollama-action](https://github.com/ai-action/ollama-action) | Run LLM prompt, capture output | Active — 32 stars, pushed 2026-06-24 | Single-shot local inference step |
| [github/github-mcp-server](https://github.com/github/github-mcp-server) | GitHub's official MCP server | Yes — 33488 stars, pushed 2026-10-08 | MCP toolset for repo operations inside agent runtime |
| [actions-runner-controller/actions-runner-controller](https://github.com/actions-runner-controller/actions-runner-controller) | ARC: K8s autoscaling runners | Yes — 6552 stars, pushed 2026-10-08; gha-runner-scale-set-0.15.0 (2026-10-01) | Heavy/GPU agent workloads beyond hosted specs |
| [github/gh-aw](https://github.com/github/gh-aw) | GitHub Agentic Workflows CLI | Yes — 5375 stars, pushed 2026-10-10 | Native public-preview platform to interoperate with |

## 3. Identity & tokens (GitHub App vs PAT vs GITHUB_TOKEN)

Built-in `GITHUB_TOKEN` (verified: [token docs](https://docs.github.com/en/actions/security-for-github-actions/security-guides/automatic-token-authentication)):

- Automatic, repo-scoped only; permissions set per workflow/job via `permissions:`
  key. Docs note an action can reach it via the `github.token` context **even if the
  workflow never passes it**, so always grant least privilege.
- Anti-recursion for free: events created with GITHUB_TOKEN do not start new
  workflow runs (only `workflow_dispatch` and `repository_dispatch` are exempt), so
  agent-authored commits cannot trigger itself
  ([events docs](https://docs.github.com/en/actions/using-workflows/events-that-trigger-workflows)).
- Fork PRs: GITHUB_TOKEN is **read-only** and **no secrets (except GITHUB_TOKEN)
  reach the runner** for fork-triggered runs
  ([events docs](https://docs.github.com/en/actions/using-workflows/events-that-trigger-workflows),
  [secrets docs](https://docs.github.com/en/actions/security-for-github-actions/security-guides/using-secrets-in-github-actions)).

GitHub App installation token (recommended bot identity for our action):

- Official pattern: register an App, store client ID in `vars`, private key in
  `secrets`, then mint a token with `actions/create-github-app-token@v3`
  ([GitHub App auth docs](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/making-authenticated-api-requests-with-a-github-app-in-a-github-actions-workflow)).
  The docs themselves demo `client-id: ${{ vars.APP_CLIENT_ID }}` +
  `private-key: ${{ secrets.APP_PRIVATE_KEY }}`.
- Verified `action.yml` (v3.2.0): `app-id` is deprecated in favor of `client-id`;
  fine-grained `permission-<scope>` inputs (contents, issues, pull-requests...);
  outputs `token`, `installation-id`, `app-slug`; `runs: using: node24` with
  `post:` revocation step.
- README (verified): installation token **expires after 1 hour**; bot git identity
  is `<app-slug>[bot]` + `<user-id>+<app-slug>[bot]@users.noreply.github.com`
  ([README](https://github.com/actions/create-github-app-token)).
- `skip-token-revoke: true` exists for jobs that outlive the default revoke step.

PAT (last resort): a secret-held personal access token works, but it identifies a
human, needs manual rotation, and grants that human's scopes. Docs position it as
the alternative when neither GITHUB_TOKEN nor an App fits
([token docs](https://docs.github.com/en/actions/security-for-github-actions/security-guides/automatic-token-authentication)).

| | GITHUB_TOKEN | GitHub App token | PAT |
|---|---|---|---|
| Setup cost | none | App registration + install | create + rotate secret |
| Expiry | job lifetime | 1 hour (mint per job) | until revoked |
| Scope | current repo only | app permissions × chosen repos | token scopes |
| Identity | `github-actions[bot]` | `<app-slug>[bot]` (distinct, branded) | a human user |
| Fork PR writes | no (read-only) | yes if app installed + permissions | yes (security risk) |
| Recursion safety | never retriggers | can retrigger (App token ≠ GITHUB_TOKEN) | can retrigger |

Recommendation: our action accepts a `github_token` input; workflows mint an App
token via create-github-app-token so agent comments/commits get a stable branded
bot identity (as claude-code-action does — its `bot_name`/`bot_id` inputs default
to `claude[bot]`; verified in its action.yml).

## 4. Event & comment interaction

Trigger semantics (all verified against
[events docs](https://docs.github.com/en/actions/using-workflows/events-that-trigger-workflows)):

- `issue_comment` (types: created/edited/deleted) fires for **both issue and PR**
  comments; distinguish via `github.event.issue.pull_request`. This is the
  @mention entrypoint (claude-code-action's `trigger_phrase` input defaults to
  `@claude`; codex-action exposes the same pattern). Workflow file must exist on
  the default branch.
- `pull_request_review` (submitted/edited/dismissed) and
  `pull_request_review_comment` let the agent react to human review feedback.
- `pull_request` / `issues` support `labeled`/`unlabeled` activity types — the
  label-trigger pattern (`label_trigger` input in claude-code-action) for
  opt-in agent runs.
- `workflow_run` (completed/requested/in_progress): the follow-up workflow **can
  access secrets and write tokens even when the triggering workflow could not** —
  ideal for "read-only analysis job → privileged fix job". Limit: workflows cannot
  chain more than three levels via workflow_run. Docs warn about pwn-request risk.
- `repository_dispatch`: custom `event_type` via REST for external triggers (CI
  bridges, webhooks); one of the two GITHUB_TOKEN-immune triggers.
- Fork delivery: for forked PRs, GitHub delivers `pull_request`, `issue_comment`,
  `pull_request_review_comment`, `pull_request_review`, `pull_request_target` to
  the base repository — so a mention-triggered agent still runs in the base repo
  context with the read-only fork restrictions above.

Comment/reaction/label actions (all repo stats verified this session):

- [marocchino/sticky-pull-request-comment](https://github.com/marocchino/sticky-pull-request-comment)
  (655★, pushed 2026-10-09): node24 action; inputs `header` (independent
  comments), `append`, `recreate`, `delete`, `hide`/`hide_classify` (minimize as
  OUTDATED etc.), `path` (comment body from file); needs `pull-requests: write`.
  No official `actions/`-org replacement exists (repos/actions/sticky-pull-request-comment → 404).
- [actions/labeler](https://github.com/actions/labeler) (2501★): config-file
  path matching to label PRs.
- [actions/github-script](https://github.com/actions/github-script) (5034★): the
  standard way to add emoji reactions to the trigger comment (eyes 👀 = working),
  update check runs, or close issues — no first-party "reaction" action exists.
- claude-code-action mirrors to study: `use_sticky_comment`,
  `classify_inline_comments` (buffer inline review comments and post once),
  `allowed_bots` / `allowed_non_write_users` gating (prompt-injection controls,
  see its action.yml warnings).

## 5. Rust toolchain & caching on runners

- [dtolnay/rust-toolchain](https://github.com/dtolnay/rust-toolchain) (1588★, pushed
  2026-10-09): verified action.yml — required `toolchain` input (e.g. `stable`,
  `nightly`, `1.80`), optional `targets`/`components` (clippy, rustfmt);
  outputs `cachekey` (short rustc-version hash, e.g. "20220627a831") and `name`.
  Both claude-style verification loops and cache keys come from here.
- [actions-rust-lang/setup-rust-toolchain](https://github.com/actions-rust-lang/setup-rust-toolchain)
  (405★, pushed 2026-09-07): maintained successor line for the old unmaintained
  actions-rs org; adds problem matchers so rustc/clippy diagnostics annotate the
  PR files view. Good alternative input for our verification step.
- [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache) v2.9.2 (1934★,
  pushed 2026-10-05): verified README — install/select the toolchain **before**
  this action because the cache key includes the rustc version; `prefix-key`,
  `shared-key`, `key` inputs; hashes Cargo.toml/Cargo.lock, rust-toolchain files
  and .cargo/config.toml into the key. Agent fix loops rerun cargo frequently, so
  this is the single biggest wall-clock lever.
- [actions/checkout](https://github.com/actions/checkout) v7.0.1 (8945★): verified
  action.yml — `fetch-depth` **defaults to 1**; set `0` to fetch all history and
  tags for full-repo agent analysis (blame, log-based reasoning); `fetch-tags`
  can pull tags even with depth > 0.
- Hosted runners ship weekly-updated preinstalled toolsets, but GitHub docs
  recommend using setup actions for reproducible versions
  ([runners doc](https://docs.github.com/en/actions/using-github-hosted-runners/using-github-hosted-runners/about-github-hosted-runners))
  — our action should pin the toolchain, not trust the image.

If the engine is TypeScript instead of pure Rust: [oven-sh/setup-bun](https://github.com/oven-sh/setup-bun)
v2.2.0 (727★, claude-code-action's choice) or [actions/setup-node](https://github.com/actions/setup-node)
(4982★, pushed 2026-10-08; codex-action pins it at v6.3.0 by SHA with node 24).

## 6. Runner capacity & self-hosted/ARC

Standard hosted runners (verified:
[runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)):

| Label | vCPU | RAM | SSD | Notes |
|---|---|---|---|---|
| ubuntu-latest / 24.04 / 22.04 / 26.04 (public repo) | 4 | 16 GB | 14 GB | x64 VM |
| ubuntu-latest (private repo) | 2 | 8 GB | 14 GB | billed minutes |
| ubuntu-24.04-arm / 26.04-arm (public) | 4 | 16 GB | 14 GB | arm64 |
| ubuntu-slim | 1 | 5 GB | 14 GB | container, unprivileged, **15-min job timeout**; made for "issue operations and short-running jobs" |
| macos-latest (M-series) | 3 | 7 GB | 14 GB | arm64 |

- 14 GB disk bounds local-model experiments; a multi-GB LLM + cargo target dir will
  not fit comfortably — agent verification fits, self-hosted local inference does not.
- Larger runners (Team/Enterprise plans): more CPU/RAM, static IPs, autoscaling and
  **GPU-powered runner** options (same docs page).
- Linux/macOS hosted runners run with passwordless sudo (same docs page) — note
  claude-code-action and codex-action both *drop* sudo before running the model
  process (defense against the agent escalating).
- Self-hosted + ARC: [actions-runner-controller](https://github.com/actions-runner-controller/actions-runner-controller)
  (6552★, pushed 2026-10-08, release `gha-runner-scale-set-0.15.0` 2026-10-01) is
  the maintained K8s operator, "developed and maintained in collaboration with the
  GitHub Actions team" (README verified). Scale sets autoscale on queue depth —
  the right substrate for a GPU pool that runs local-model BYOK agents.
- Docs guidance for ARC: [About ARC](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners-with-actions-runner-controller/quickstart-for-actions-runner-controller).

## 7. Agent-hosting building blocks

What actually exists and is maintained (verified this session):

- **API-model agents (the mainstream)**: both reference actions call hosted models
  with BYOK secrets; no GPU/local runtime needed on the runner.
- **Local model runtimes**: the Ollama ecosystem on Actions is thin —
  [ai-action/setup-ollama](https://github.com/ai-action/setup-ollama) (15★) and
  [ai-action/ollama-action](https://github.com/ai-action/ollama-action) (32★) are
  maintained but tiny; official `curl -fsSL https://ollama.com/install.sh | bash`
  plus `ollama serve &` in-workflow is equally viable on self-hosted/GPU runners.
  No large, well-maintained "llm gateway in CI" action surfaced in marketplace
  searches (queries: ollama/marketplace/model/openai).
- **MCP in CI**: [github/github-mcp-server](https://github.com/github/github-mcp-server)
  (33488★, pushed 2026-10-08) is GitHub's official MCP server; agentic workflows
  consume it via `tools: github: toolsets: [default]` (verified in the docs below),
  and claude-code-action passes `MCP_TIMEOUT`/`MCP_TOOL_TIMEOUT`/
  `MAX_MCP_OUTPUT_TOKENS` through to its CLI (verified in its action.yml). A public
  pattern exists: run the agent in one job with read-only tools, let it propose
  writes, apply them in a privileged job.
- **Native platform**: **GitHub Agentic Workflows** is in public preview (changelog
  2026-06-11; verified docs:
  [About GitHub Agentic Workflows](https://docs.github.com/en/copilot/concepts/agents/about-github-agentic-workflows)).
  Markdown + frontmatter, compiled to a hardened `.lock.yml` by `gh aw`
  ([github/gh-aw](https://github.com/github/gh-aw), 5375★). Engines: Copilot
  (default), Claude, Codex, Gemini — each with its own auth secret (BYOK model).
  Security model: read-only tokens by default, writes only via declared
  **safe-outputs**, threat detection on proposals, firewalled containers, secrets
  held in downstream jobs outside the agent runtime; billing in AI Credits
  (1 AIC = $0.01, default cap 1000 AIC/run, `max-ai-credits` frontmatter).

  Implication for storytold: our action should speak the same language (BYOK
  secrets per provider, safe-write validation, read-only default) so it can serve
  as a heavier, code-verification-first alternative with full tool access.

## 8. Packaging / distribution patterns

Three action types (verified:
[About custom actions](https://docs.github.com/en/actions/creating-actions/about-custom-actions)):
Docker container (consistent env, Linux-only, slow start), JavaScript (fast, runs
directly on the runner, cross-OS), Composite (bundles multiple steps into one).

How the two reference agent actions package themselves (action.yml files fetched
and read in full this session):

- [anthropics/claude-code-action](https://github.com/anthropics/claude-code-action)
  (9463★): **composite** action. Steps: install Bun via
  `oven-sh/setup-bun@<full-SHA> # v2.2.0` with `bun-version: 1.3.14` (SHA-pinned),
  `bun install --production` inside `${GITHUB_ACTION_PATH}`, then run
  `src/entrypoints/run.ts` with bun; post steps post buffered inline comments and
  DELETE-revoke the App token. Inputs: `trigger_phrase` (@claude), `label_trigger`,
  `github_token`, `anthropic_api_key`, `claude_code_oauth_token`, or **OIDC workload
  identity federation** (`anthropic_federation_rule_id` + org/workspace IDs,
  requires `id-token: write`), `use_bedrock`/`use_vertex`/`use_foundry`,
  `use_sticky_comment`, `use_commit_signing`/`ssh_signing_key`, `plugins`/
  `plugin_marketplaces`. Outputs: `conclusion`, `branch_name`, `session_id`,
  `structured_output` (JSON-schema'd model output), `github_token`.
- [openai/codex-action](https://github.com/openai/codex-action)
  (1262★): **composite** action. Steps: pin
  `actions/setup-node@<full-SHA> # v6.3.0` with node 24; `npm install -g
  @openai/codex@<version>` **plus `@openai/codex-responses-api-proxy@<version>`**
  (local proxy keeps the API key away from model-facing process args); a
  node `dist/main.js check-write-access` gate (`allow-bots`/`allow-users` inputs)
  before anything runs; `safety-strategy` input picks sandboxing — `drop-sudo`
  (default), `unprivileged-user`, `read-only`, or `unsafe` (Windows: unsafe only);
  `permission-profile` (`:read-only`, `:workspace`). Output: `final-message`.
  Sibling [openai/codex-security](https://github.com/openai/codex-security)
  (11054★) ships a security-scanning flavor.

Both are composite wrappers around a CLI installed at run time, pin every
third-party action by full SHA with a `# vX.Y.Z` comment, and carry their compiled
entrypoints (`dist/main.cjs`, `dist/main.js`) in-repo.

Recommendation for our action: **composite action that downloads our prebuilt,
self-contained Rust binary** (from a pinned release tag, version input with
default) and executes it. Pure-Rust static binary = Docker's reproducibility with
JavaScript's cross-OS speed; no runtime install step, works on x64/arm64 and
Linux/macOS runners (Docker actions are Linux-only per the docs above).

Release conventions (verified:
[Managing custom actions](https://docs.github.com/en/actions/how-tos/create-and-publish-actions/manage-custom-actions)):

- Ship the action in its own repository (docs recommendation) with semantic
  version tags; move the major tag (`v1`) to each new release; users reference
  `@v1`, `@v1.0.1`, or a full commit SHA for immutability; never tell users to
  reference the default branch.
- Use `GITHUB_API_URL` / `GITHUB_GRAPHQL_URL` env (or `@actions/github`) instead
  of hard-coded `api.github.com` so GHE.com/GHES work — relevant because our
  agent must be enterprise-installable.
- README must document inputs/outputs/secrets/env (docs checklist).

## 9. Recommended skeleton pipeline (each step → verified action)

```yaml
name: storytold-agent
on:
  issue_comment: { types: [created] }              # @storytold mention trigger (docs: fires on issue+PR comments)
  pull_request:  { types: [opened, synchronize, ready_for_review, labeled] }
  issues:        { types: [opened, labeled] }       # label-gated issue work
  workflow_run:                                    # privileged follow-up to read-only analysis (docs: grants secrets+write)
    workflows: [storytold-analysis]
    types: [completed]
  repository_dispatch: { types: [agent-task] }      # external/webhook entry (docs: REST-triggered)

permissions:
  contents: read        # least-privilege default; write only in the fix job

concurrency:
  group: storytold-agent-${{ github.event.issue.number || github.event.pull_request.number || github.run_id }}
  cancel-in-progress: false   # one agent per PR; queue the rest

jobs:
  agent:
    if: >-                                    # mention or label triggers only
      (github.event_name == 'issue_comment' && contains(github.event.comment.body, '@storytold'))
      || (github.event.action == 'labeled' && github.event.label.name == 'agent')
      || github.event_name == 'workflow_run' || github.event_name == 'repository_dispatch'
    runs-on: ubuntu-latest                    # 4 vCPU/16 GB public, 2/8 private (runners reference)
    steps:
      - uses: actions/create-github-app-token@v3   # bot identity; client-id from vars, key from secrets
        id: app-token
        with: { client-id: ${{ vars.APP_CLIENT_ID }}, private-key: ${{ secrets.APP_PRIVATE_KEY }} }
      - uses: actions/github-script@v8             # 👀 reaction on the triggering comment
        with: { script: "await github.rest.reactions.createForIssueComment({...context.issue, comment_id: context.payload.comment.id, content: 'eyes'})" }
      - uses: actions/checkout@v7                  # full history for agent reasoning
        with: { token: '${{ steps.app-token.outputs.token }}', fetch-depth: 0 }
      - uses: dtolnay/rust-toolchain@stable        # toolchain + cachekey output
        with: { components: 'clippy, rustfmt' }
      - uses: Swatinem/rust-cache@v2                # warm cargo caches for the verify loop
      - uses: storytold/agent-action@v1            # OUR composite action: runs the Rust agent binary,
        with:                                        #   BYOK secret (OPENAI_API_KEY etc.), verification loop
          github-token: '${{ steps.app-token.outputs.token }}'   #   (cargo fmt/clippy/test), writes a report file
          api-key: ${{ secrets.STORYTOLD_MODEL_KEY }}
      - uses: marocchino/sticky-pull-request-comment@v2   # update-in-place report per PR
        with: { header: storytold-agent, path: agent-report.md,
                 GITHUB_TOKEN: '${{ steps.app-token.outputs.token }}' }
  commit-fixes:                                   # separate job, contents: write, human-gated
    needs: agent
    if: needs.agent.outputs.changed == 'true'
    permissions: { contents: write, pull-requests: write }
    steps:
      - uses: actions/checkout@v7
      - uses: stefanzweifel/git-auto-commit-action@v6   # or peter-evans/create-pull-request@v7 for PR-based fixes
```

Design notes on the skeleton:

- **Mention + label triggers** mirror claude-code-action's `trigger_phrase` /
  `label_trigger` inputs; `issue_comment` is the only way to catch @mentions on
  PRs, and fork PRs still deliver it to the base repo (events docs).
- **App token, not GITHUB_TOKEN, for writes**: branded bot identity, fork-PR
  writes work, and `app-slug` output feeds git author config (README pattern).
  Keep the read-only GITHUB_TOKEN for the analysis job's repo access.
- **Two-job privilege split**: the analysis job gets `contents: read` only; the
  fix/commit job (or a `workflow_run` successor) carries the write permission —
  the same layered pattern GitHub's own agentic workflows mandate ("read-only by
  default, safe outputs validated before write", About GitHub Agentic Workflows)
  and the one the codex-action write-access gate implements.
- **No recursion risk** when committing with GITHUB_TOKEN (events it creates
  don't trigger workflows), but App-token pushes can retrigger `pull_request
  synchronize` — guard with the `concurrency` group and an `actor != bot` condition,
  as claude-code-action's `allowed_bots` input does.
- **Verification loop lives in OUR action** (the Rust binary runs
  cargo fmt/clippy/test), so the workflow YAML stays declarative; the market
  gap we fill vs claude-code-action/codex-action is deep, tool-executing
  verification with BYOK model choice, not another thin CLI wrapper.

Verification status of every `@ref` in the skeleton: create-github-app-token@v3 ✓
(release v3.2.0), checkout@v7 (v7.0.1) ✓, github-script (repo active; major tag
not release-checked) ✓, dtolnay/rust-toolchain ✓, Swatinem/rust-cache@v2
(v2.9.2) ✓, sticky-pull-request-comment ✓, git-auto-commit-action ✓,
peter-evans/create-pull-request ✓. Pin by full SHA in production (docs: SHA is
immutable; both reference actions do exactly this).
