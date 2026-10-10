# Claude bots on GitHub — Anthropic official offerings (ws2-claude-bots)

Researched 2026-10-10 for the storytold agentic-GitHub-Action project.
Every claim below was verified this session against primary sources: the
anthropics/claude-code-action repo (README, action.yml, docs/) via
raw.githubusercontent.com and the GitHub API, and Anthropic docs at
code.claude.com/docs. Source URLs are cited inline per claim.

## 1. Methodology (searches run)

Web searches (Google-backed MCP):
- "claude code github action setup 2026 @claude bot anthropic"
- "claude code action fork non-anthropic model openrouter openai proxy github action"
- "claude code --dangerously-skip-permissions flag 2026 deprecated bypassPermissions"
(A 4th search, on run-concurrency limits, failed on a stale OAuth token;
concurrency guidance was instead verified directly on the official
GitHub Actions docs page.)

Direct verifications (12 page/repo fetches + GitHub API calls):
- gh api repos/anthropics/claude-code-action (metadata, 2026-10-10)
- raw README.md and action.yml from anthropics/claude-code-action HEAD
- repo docs/: setup.md, usage.md, faq.md, security.md,
  capabilities-and-limitations.md, cloud-providers.md
- https://code.claude.com/docs/en/github-actions
- https://code.claude.com/docs/en/headless (run Claude Code programmatically)
- https://code.claude.com/docs/en/iam (authentication + credential precedence)
- https://code.claude.com/docs/en/permission-modes
- GitHub API repo lookups for forks/adaptations: grll/claude-code-action,
  markwylde/claude-code-gitea-action, musistudio/claude-code-router,
  1rgs/claude-code-proxy, luohy15/y-router, anthropics/claude-code-base-action,
  anthropics/claude-code-security-review (plus a repo search + 3 READMEs)

## 2. The @claude bot / Claude Code GitHub Action — setup & auth matrix

The product today is Claude Code Action v1.0 (anthropics/claude-code-action):
a general-purpose action for PRs and issues that answers questions and
implements code changes, running entirely on the user's own GitHub runner.
MIT-licensed, 9,463 stars, last pushed 2026-10-09, not archived. It is one
of a family: the action (workflow-based), Code Review (hosted, automatic PR
review with no workflow file), Claude Code in the cloud, the Agent SDK (which
the action is built on), and a GHES variant. Lower-level building block:
anthropics/claude-code-base-action.
Sources: https://github.com/anthropics/claude-code-action (gh api, 2026-10-10);
https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/README.md;
https://code.claude.com/docs/en/github-actions

### Setup paths
- Quick: run /install-github-app inside the claude CLI — installs the Claude
  GitHub App, stores the auth secret, pushes a workflow PR. Repo admin
  required; github.com remotes only.
- Manual: install the app (https://github.com/apps/claude), add the auth
  secret(s), copy the example workflow into .github/workflows/. Repo admin
  required. Custom GitHub App path is documented for orgs that will not
  install third-party apps (manifest + APP_ID/APP_PRIVATE_KEY secrets +
  actions/create-github-app-token).
- Org rollout: install the app once at org level, keep the auth secret as an
  org-level Actions secret, add the workflow per repo or share one reusable
  workflow. Prefer an API key over an OAuth token for shared secrets, since
  the OAuth token is tied to the subscription of whoever ran claude setup-token.
Sources: https://code.claude.com/docs/en/github-actions;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/setup.md

### Auth matrix — model access (exactly one of these)

| Path | Inputs / secrets | Notes |
|---|---|---|
| API key (direct) | anthropic_api_key (ANTHROPIC_API_KEY secret) | Console pay-per-token billing |
| OAuth token | claude_code_oauth_token (CLAUDE_CODE_OAUTH_TOKEN) | one-year token from claude setup-token; bills to a Pro/Max/Team/Enterprise subscription |
| Workload identity federation | anthropic_federation_rule_id (fdrl_...) + anthropic_organization_id (+ optional svac_/wrkspc_ service account/workspace, anthropic_oidc_audience) | secretless: workflow's GitHub OIDC token exchanged for short-lived Claude API access; needs id-token: write; identifiers are not credentials; inline-comment classification is skipped under WIF |
| Amazon Bedrock | use_bedrock: "true" + OIDC via aws-actions/configure-aws-credentials | no static cloud creds; model set via claude_args |
| Google Vertex AI / Agent Platform | use_vertex: "true" + OIDC via google-github-actions/auth | e.g. claude-4-0-sonnet@20250805 |
| Microsoft Foundry | use_foundry: "true" + OIDC via azure/login + ANTHROPIC_FOUNDRY_BASE_URL | newest provider path |

Sources: https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/action.yml;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/setup.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/cloud-providers.md

### Auth matrix — GitHub side (how the bot talks to GitHub)

- Default: the action authenticates as the official Claude GitHub App, minting
  a short-lived repo-scoped installation token that it revokes at the end of
  the run; the workflow must grant id-token: write. Comments appear as
  claude[bot] (default bot_id 41898282, overridable via bot_id/bot_name).
- Alternative: github_token input — a custom GitHub App token (minimum
  permissions Contents/Issues/Pull requests RW) or a PAT.
- The official app's requested permission set is shared across all Claude
  GitHub features and is broad (Contents, PRs, Issues, Actions, Checks,
  Discussions, Workflows RW + read-only perms); the action's security doc
  states only Contents/PRs/Issues RW are currently used, the rest being for
  sibling/future features. A minimal custom app is the documented mitigation.
Sources: https://code.claude.com/docs/en/github-actions;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/setup.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/security.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/action.yml

## 3. Triggers & capabilities

### Trigger model
- Interactive mode (no prompt input set): waits for the trigger phrase
  (default @claude, word-boundary match) in issue/PR comments, PR review
  comments/submissions, and new issue bodies/titles; plus assignee_trigger
  and label_trigger (default label "claude"). trigger_phrase is configurable.
- Automation mode (prompt input set): runs immediately on any GitHub event,
  including cron schedules; the prompt can be plain text or a skill
  invocation (/skill-name, or /plugin-name:skill-name with plugins installed).
- Typical wired events (usage example): issue_comment (created),
  pull_request_review_comment (created), issues (opened, assigned, labeled),
  pull_request_review (submitted); the solutions guide adds PR-event and
  scheduled automations (auto-review, path-specific review, triage,
  maintenance).
Sources: https://code.claude.com/docs/en/github-actions;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/usage.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/README.md

### Who can trigger it
- Write access required for issue/PR/comment/review actors; workflow_dispatch,
  repository_dispatch, and schedule are not separately checked (GitHub already
  gates them); workflow_run checks the upstream run's actor too.
- Bot actors are rejected unless listed in allowed_bots; on public repos
  allowed_bots "*" lets any GitHub App invoke the action with prompts it
  controls (documented warning).
- allowed_non_write_users bypasses the write check, only works with a
  github_token input, and triggers best-effort secret scrubbing plus
  bubblewrap/socat PID-namespace subprocess isolation on Linux; documented
  as significant risk for minimal-permission workflows only.
Sources: https://code.claude.com/docs/en/github-actions;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/security.md

### Capabilities
- Answers code/architecture questions; implements code changes; reviews PRs
  (with the code-review skill + --comment it posts inline comments per finding
  or one summary comment); reads CI runs/logs/test results when granted
  actions: read via additional_permissions; sees uploaded images.
- Branch handling: issue trigger always creates a new branch (default prefix
  claude/, name template with {prefix}/{entityType}/{entityNumber}/{timestamp}/
  {sha}/{label}/{description}); open PR pushes directly to the PR branch;
  closed PR gets a new branch.
- Progress UI: updates a single comment with checkboxes; use_sticky_comment
  forces one-comment delivery; track_progress adds tracking comments.
- Action outputs: conclusion, execution_file, branch_name, github_token,
  structured_output (validated JSON via --json-schema), session_id (usable
  with claude --resume to continue the conversation).
Sources: https://code.claude.com/docs/en/github-actions;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/capabilities-and-limitations.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/action.yml

## 4. Configuration (CLAUDE.md and friends)

- CLAUDE.md at the repo root: coding standards, review criteria, project
  rules. Read on every run, so the official docs advise keeping it concise.
- .claude/ directory: settings (the settings input takes a JSON string or a
  path to a settings JSON file), skills in .claude/skills/ invocable as the
  prompt, slash commands, agents.
- Plugins: plugin_marketplaces + plugins inputs install marketplaces/plugins
  before the run (e.g. code-review@claude-code-plugins); skills packaged in
  plugins run as /plugin-name:skill-name.
- claude_args: raw passthrough of any Claude Code CLI flag — --max-turns,
  --model (e.g. claude-sonnet-5), --allowedTools / --allowed-tools,
  --disallowedTools, --mcp-config, --append-system-prompt, --system-prompt,
  --debug, --json-schema, and more.
- v1 deprecated inputs (see migration guide): mode, direct_prompt,
  override_prompt, custom_instructions, max_turns, model, fallback_model,
  allowed_tools, disallowed_tools, mcp_config, claude_env — replaced by
  prompt, claude_args, or settings.
Sources: https://code.claude.com/docs/en/github-actions;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/usage.md

### Tool permission model
- Bash is disabled by default; enable narrowly via claude_args
  --allowedTools "Bash(npm:*),Bash(git:*)" (permission-rule syntax, prefix
  match with trailing space-star).
- The action auto-configures two MCP servers (GitHub API + file operations);
  their tools still require explicit --allowedTools entries.
- CLI permission modes: default (Manual), acceptEdits, plan, auto
  (classifier-reviewed actions; built-in starting mode on v2.1.283+), dontAsk
  (locked-down CI: anything that would prompt is denied), bypassPermissions
  (everything — isolated containers/VMs only).
- --permission-prompts none for unattended runs: no permission host is
  consulted; denials appear as permission_denied stream events.
Sources: https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/faq.md;
https://code.claude.com/docs/en/permission-modes;
https://code.claude.com/docs/en/headless

### Anti-injection config handling on PRs
Before starting Claude on a PR, the action restores a fixed list of agent
config paths from the PR base branch: .claude/, .mcp.json, .claude.json,
.gitmodules, .ripgreprc, CLAUDE.md, CLAUDE.local.md, .husky/. PR-authored
versions are kept under .claude-pr/ for reference only; everything else in
the tree stays at the PR head. Docs warn hooks/apiKeyHelper commands must
stay self-contained because the runtime resolves through PR-supplied files.
Source: https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/security.md

### Self-hosted runners & custom environments
- Runs on hosted or self-hosted runners (inference calls go to your chosen
  provider from your runner). Subprocess-isolation dependencies
  (bubblewrap/socat) install best-effort; skipped on non-Linux or
  self-hosted runners without apt/sudo.
- path_to_claude_code_executable and path_to_bun_executable inputs target
  Nix, custom containers, and pinned versions (the action otherwise installs
  Bun 1.3.14 and Claude Code itself).
Sources: https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/README.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/action.yml

## 5. BYOK & model routing

BYOK is the default model for the action: you supply ANTHROPIC_API_KEY (or an
OAuth subscription token, or cloud-provider OIDC) as a repo/org secret; no
Anthropic-hosted component runs the model — inference happens from your
runner to the provider you chose.
Sources: https://code.claude.com/docs/en/github-actions;
https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/README.md

### Alternative providers inside the official action
- Official cloud providers: Bedrock / Vertex (Agent Platform) / Foundry via
  use_bedrock | use_vertex | use_foundry with OIDC — no static cloud
  credentials stored in the repo.
- Official custom-endpoint hook: action.yml forwards ANTHROPIC_BASE_URL and
  ANTHROPIC_CUSTOM_HEADERS from the workflow env to the CLI step; the IAM
  docs state ANTHROPIC_BASE_URL routes requests through a custom API
  endpoint, and ANTHROPIC_AUTH_TOKEN (Bearer header) exists for LLM
  gateways/proxies. Managed settings even sanction customEndpoint as a
  provider class via allowedProviders. The endpoint must still speak the
  Anthropic Messages API — pointing directly at OpenAI/OpenRouter does not
  work without a translating proxy.
Sources: https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/action.yml;
https://code.claude.com/docs/en/iam

### Headless CLI patterns for CI
- claude -p "<prompt>": non-interactive; exit code 0 on success, non-zero on
  failure; pipes stdin/stdout like any Unix tool.
- --output-format text | json | stream-json. json includes total_cost_usd and
  a per-model cost breakdown; stream-json emits newline-delimited events:
  system/init (session metadata, loaded plugins/MCP), api_retry,
  permission_denied, plugin/MCP load errors a CI gate can fail on.
- --json-schema returns schema-validated structured output.
- --bare mode for CI: skips hooks, skills, custom commands, subagents,
  plugins, MCP servers, auto memory, and CLAUDE.md for reproducible runs;
  requires ANTHROPIC_API_KEY (or apiKeyHelper) because bare mode never reads
  OAuth credentials or the OS keychain.
- Sessions: --continue (latest) / --resume <session-id or transcript .jsonl>;
  the action exports session_id for exactly this.
- Autonomy: --allowedTools allowlists; --permission-mode dontAsk for
  locked-down CI; --dangerously-skip-permissions still exists but maps to
  bypassPermissions and is documented as requiring a container, VM, or
  sandbox runtime plus a non-root user. --max-budget-usd caps run spend.
- Hooks: a -p session runs project hooks from .claude/settings.json and
  connects .mcp.json servers unless --bare; SIGTERM exits 143 and runs
  SessionEnd hooks; PermissionRequest hooks can auto-allow requests even
  under --permission-prompts none.
Sources: https://code.claude.com/docs/en/headless;
https://code.claude.com/docs/en/permission-modes

### Community BYOK/proxy patterns
- The community pattern for non-Anthropic models is a translation proxy that
  implements the Anthropic Messages API over OpenAI-compatible backends,
  pointed at via ANTHROPIC_BASE_URL plus a provider key in
  ANTHROPIC_API_KEY / ANTHROPIC_AUTH_TOKEN.
- Verified tools (GitHub API, 2026-10-10): musistudio/claude-code-router
  (37,609 stars, multi-provider routing, pushed 2026-10-10);
  1rgs/claude-code-proxy (3,753 stars, LiteLLM proxy — "Run Claude Code on
  OpenAI models", also Gemini); luohy15/y-router (383 stars, OpenRouter).
Sources: https://github.com/musistudio/claude-code-router;
https://github.com/1rgs/claude-code-proxy; https://github.com/luohy15/y-router

## 6. Pricing model

- The action itself is free and MIT-licensed; there is no Anthropic charge
  for the bot software.
- Compute: your GitHub Actions minutes (hosted runners) or your own
  self-hosted runners.
- Model access, pick one: API key → Console pay-per-token billing; OAuth
  token → Claude subscription usage (Pro/Max/Team/Enterprise); workload
  identity federation → Console org billing with no stored secret;
  Bedrock/Vertex/Foundry → billed through your AWS/GCP/Azure account.
- Cost guardrails recommended by the official docs: write specific requests,
  use issue templates, keep CLAUDE.md concise (read every run), set
  --max-turns, set workflow-level timeouts, and use GitHub concurrency
  controls to cap parallel runs. CLI-level: --max-budget-usd and
  total_cost_usd reporting.
Sources: https://code.claude.com/docs/en/github-actions;
https://code.claude.com/docs/en/headless

## 7. Limitations

- Cannot submit formal GitHub PR reviews; cannot approve PRs (deliberate).
- Cannot post multiple comments — updates one comment (sticky-comment mode
  only works with claude[bot] auth).
- Does not create PRs by default: pushes a branch and links a prefilled PR
  creation page (branch-protection friendly, human stays in the loop).
- No merge/rebase/force-push — enforced in the system prompt even if the git
  tools are explicitly allowed; never pushes outside the invoking branch.
- Will not edit workflow files (app's workflow write is withheld).
- Bash disabled by default; repo-scoped token with no cross-repo access.
- Shallow clones: --depth=20 on PRs, --depth=1 for new branches (tunable via
  actions/checkout).
- The github-actions actor cannot re-trigger workflows (GitHub loop
  prevention) — chained automation needs a PAT or separate app token;
  passing GITHUB_TOKEN as github_token also suppresses CI on Claude's pushes.
- Prompt-injection surface is documented: input sanitization strips HTML
  comments, invisible characters, markdown image alt text, hidden
  attributes; base-branch config restore (see section 4);
  include/exclude_comments_by_actor filters whose comments reach Claude.
- Inline-comment classification (real review vs probe) uses a Haiku pass
  and requires anthropic_api_key; skipped under federation auth.
- No built-in run queueing — use GitHub Actions concurrency to limit parallel
  runs; FAQ advises monitoring token usage to avoid API limits.
- Private org repos: @claude cannot be assigned to issues (assignees must be
  org members); documented workaround is a custom user.
Sources: https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/capabilities-and-limitations.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/faq.md;
https://raw.githubusercontent.com/anthropics/claude-code-action/main/docs/security.md;
https://code.claude.com/docs/en/github-actions

## 8. Notable forks / adaptations

- markwylde/claude-code-gitea-action (MIT, 164 stars, pushed 2026-08-07): a
  port of the action to Gitea — same trigger-phrase/PR/issue interaction
  model, but driven by a GITEA_TOKEN secret and local git operations
  because of Gitea's API limits; auth via API key, Bedrock, or Vertex.
  Concrete proof the architecture ports to non-GitHub forges.
  Source: https://github.com/markwylde/claude-code-gitea-action (gh api + README)
- grll/claude-code-action (457 stars): historically added OAuth login so
  Claude Max subscribers could use their subscription in CI (with token
  refresh via a PAT and a companion claude-code-login action). The author
  declared it obsolete on 2025-07-08, after Anthropic shipped native OAuth
  and /install-github-app. Lesson: gap-filling forks get absorbed upstream.
  Source: https://github.com/grll/claude-code-action (gh api + README)
- anthropics/claude-code-base-action (999 stars): official lower-level
  building block — installs and runs Claude Code with your inputs, without
  actor permission checks or base-branch config restore. For custom
  pipelines that want raw Claude Code in a workflow step.
  Source: https://github.com/anthropics/claude-code-base-action (gh api)
- anthropics/claude-code-security-review (6,331 stars, pushed 2026-02-11):
  official sibling action focused on security review of code changes.
  Source: https://github.com/anthropics/claude-code-security-review (gh api)
- Model-swapping is done at the proxy/router layer (claude-code-router,
  claude-code-proxy, y-router — see section 5), not by forking the action:
  this session's repo search found no maintained fork that swaps the model
  layer, which the ANTHROPIC_BASE_URL/ANTHROPIC_AUTH_TOKEN env passthrough
  makes unnecessary.
  Sources: https://github.com/search?q=claude-code-action+fork:true (gh api,
  2026-10-10); https://raw.githubusercontent.com/anthropics/claude-code-action/HEAD/action.yml

## 9. Implications for our project (storytold agentic Rust GitHub Action)

### What to copy
- Auth shape: GitHub App with short-lived installation tokens (revoked
  post-run) as the default; a custom-app path with a shipped manifest for
  orgs that refuse third-party apps; secrets only for model keys. Keep the
  app's permission set minimal — Claude's broad shared permission set is a
  documented criticism they mitigate via custom apps.
- Dual-mode auto-detection: mention-triggered interactive mode vs
  prompt-driven automation mode on any event/cron. One action, two
  behaviors, no explicit mode flag.
- Trigger gating: write-access check + bot rejection by default, with narrow,
  loudly-warned escape hatches (allowed_bots, allowed_non_write_users).
- Human-in-the-loop PR flow: push a branch, link a prefilled PR page; never
  auto-open PRs, approve, or force-push — enforced in the agent's system
  prompt, not just tool config.
- Single sticky comment with progress checkboxes: low-noise UX.
- Base-branch restore of agent config on PRs: the standout anti-injection
  mechanism; copy it.
- Raw CLI passthrough (claude_args-style) + settings JSON + structured
  outputs as action outputs + session_id for resumability.
- CLAUDE.md-style repo memory file + skills directory: cheap, high leverage.
- Honest capabilities/limitations and FAQ docs: stating what the bot will
  NOT do is a trust feature.

### What to do differently
- Single static Rust binary: claude-code-action installs Bun 1.3.14 plus the
  Claude CLI at run time. A pure-Rust agent ships as one static musl binary —
  faster, cacheable, no runtime deps, runs on minimal self-hosted runners.
  Real differentiator for a Rust org.
- BYOK-native, multi-provider from day one: Claude treats alternative
  providers as pass-through env vars and needs third-party proxies for
  OpenAI-compatible endpoints. Make provider config first-class action
  inputs (anthropic key / openai-compatible key + base URL + model id) and
  support OpenAI chat-completions natively — no proxy for users to deploy.
- Declarative verification config: our cargo test/clippy/rustfmt loop
  deserves a checked-in config (e.g. verify.toml with build/test/lint
  commands) the agent runs via a narrow allowlist, instead of users
  hand-writing Bash(...:*) allowlists in workflow YAML.
- Rust-aware verification loop as the core workflow: run verification, read
  failures, fix, re-run. Claude's action has no built-in verify step — it
  relies on generic bash plus actions: read.
- Forge portability: the Gitea port proves demand; separating GitHub-API
  glue from the agent core makes a Gitea/Forgejo variant cheap.
- Org rollout story: for ~22 repos, document org-level secrets + a reusable
  workflow as the primary install path (exactly the pattern Claude documents).
- Cost guardrails as defaults, not footnotes: max-turns, budget cap,
  timeouts, GitHub concurrency in the README quickstart.
- Watch-outs from Claude's experience: GITHUB_TOKEN pushes do not trigger
  CI (decide early whether the bot pushes via app token and document it);
  OAuth-style subscription auth is a vendor retention feature an OSS BYOK
  project cannot replicate — but the WIF/OIDC secretless pattern is worth
  remembering if storytold ever hosts a control plane.

---
All repo metadata (stars, push dates, licenses) was read live on 2026-10-10
via gh api; repo docs were read from raw.githubusercontent.com at HEAD of
main; doc-page claims come from code.claude.com/docs pages fetched this
session. Search tool: agy_web_search (Google-backed) for discovery only —
every fact above is grounded in a fetched primary source.
