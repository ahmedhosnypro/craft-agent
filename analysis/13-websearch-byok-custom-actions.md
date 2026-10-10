# BYOK custom AI-agent GitHub Actions + security rules (ws4-byok-custom)

Date: 2026-10-10. Context: designing an open-source agentic AI GitHub Action for the `storytold` org. Scope: real BYOK AI actions/bots, self-built agent-action write-ups, BYOK security rules verified against docs.github.com, provider-agnostic model access, recommended secret/identity architecture.

## 1. Methodology

Searches run (Google-backed `agy_web_search` + GitHub API `search/repositories` + `gh api` per-repo verification):

| # | Query / probe | Tool |
|---|---|---|
| 1 | github action llm api key secret BYOK AI review | agy web search |
| 2 | openrouter github action AI agent API key | agy web search |
| 3 | claude github action / ai code review action / codex github action / openrouter+deepseek+grok+ollama actions | gh api repo search (star-sorted) |
| 4 | "we built" github action AI coding agent engineering blog | agy web search |
| 5 | github actions security pull_request_target pwn request fork PR secrets prompt injection | agy web search |
| 6 | Cloudflare Astro "software factory" issue triage | agy web search |
| 7 | triagebot-action | gh api repo search |
| 8 | openhands resolver | gh api repo search |

Verification method: every action below was confirmed with `gh api repos/OWNER/REPO` (stars, `pushed_at`, `archived`) plus its raw README via `gh api repos/OWNER/REPO/readme` on 2026-10-10. GitHub security claims were verified by fetching the official docs pages listed inline (FetchURL/curl of docs.github.com). The Cloudflare write-up was verified by fetching the blog post in full. Where a claim could not be verified from a primary source it is either omitted or explicitly marked.

## 2. Verified BYOK AI action/bot examples

Stars and `pushed_at` verified via `gh api repos/...` on 2026-10-10. "Agentic" = multi-step tool use (reads repo, runs commands, iterates); "single-shot" = one model call over given context.

| Action | What it does | Agentic? | Key passing (BYOK) | Maintained |
|---|---|---|---|---|
| `anthropics/claude-code-action` (9,463★) | General-purpose Claude Code agent for PRs/issues; answers mentions, implements changes, creates branches | Agentic | `ANTHROPIC_API_KEY` secret, or `CLAUDE_CODE_OAUTH_TOKEN` (Pro/Max sub via `claude setup-token`), or Bedrock/Vertex/Foundry, or OIDC Workload Identity Federation (no static key) | Yes — pushed 2026-10-09 |
| `anthropics/claude-code-security-review` (6,331★) | Security-focused review of PR changes via Claude Code `/security-review` | Single-shot analysis | `claude-api-key: ${{ secrets.CLAUDE_API_KEY }}` (key must be enabled for Claude API + Claude Code) | Last push 2026-02-11 |
| `openai/codex-action` (1,262★, official) | Runs Codex CLI (`codex exec`) in workflows behind a secure proxy to the Responses API; permission profiles | Agentic | `openai-api-key: ${{ secrets.OPENAI_API_KEY }}` or `AZURE_OPENAI_API_KEY`; key stays in the proxy, not the agent env | Yes — pushed 2026-10-09 |
| `anc95/ChatGPT-CodeReview` (4,466★) | PR code review + inline comments | Single-shot (diff-to-prompt) | `OPENAI_API_KEY` secret; optional `OPENAI_API_ENDPOINT` + `MODEL` inputs | Yes — pushed 2026-08-10 |
| `aeonfun/aeon` (770★) | Autonomous agent framework running unattended on Actions; drives Claude Code, Grok, Codex + 6 other harnesses, self-healing skills | Agentic (heavily) | Keys stored in repo encrypted secrets: Claude subscription token, plain API key, ChatGPT login, OpenRouter one-click; `XAI_API_KEY` etc. | Yes — pushed 2026-10-10 |
| `truongnh1992/gemini-ai-code-reviewer` (254★) | PR review with Gemini | Single-shot | Gemini API key from Google AI Studio → repo secret | Stale — last push 2025-12-16 |
| `withastro/triagebot-action` (222★) | Astro issue-triage "software factory": reproduce → diagnose → verify → fix, preview releases | Agentic (subagent pipeline) | `read-token` GITHUB_TOKEN + `write-token` BOT_GITHUB_TOKEN + `cloudflare-api-key`/`account-id` + `triage-model`/`verification-model` strings | Yes — pushed 2026-08-28 |
| `sturdy-dev/codeball-action` (326★) | AI PR scoring/approval; the action calls the hosted codeball.ai service | n/a (SaaS) | n/a — no BYOK model key; included to show the SaaS-vs-BYOK contrast | Dead — last push 2023-04 |
| `tarmojussila/zai-code-review` (43★) / `xai-code-review` (17★) | PR review with Z.ai GLM / xAI Grok | Single-shot | `ZAI_API_KEY` / `XAI_API_KEY` repo secrets + model input | Yes — pushed 2026-09-16 |
| `appleboy/LLM-action` (27★) | One action for any OpenAI-compatible service (OpenAI, Azure, Ollama, LocalAI, vLLM); Go-template prompts, structured output via function calling | Single-shot | `api_key` (required) + `base_url` input (default `https://api.openai.com/v1`), `model`; custom CA certs and custom headers for air-gapped self-host | Yes — pushed 2026-10-06 |
| `Lixiaoyiao/deepseek-harness-action` (22★) | DeepSeek Harness agent: PR review, CI diagnosis, auto-fix, issue→PR | Agentic | DEEPSEEK key via secrets; runs a credential-isolated worker, trusted Controller publishes | Yes — pushed 2026-10-07 |
| `erans/autoagent-action` (17★) | Composable action wrapping Cursor CLI, Claude Code, Gemini CLI, Codex CLI, Amp, OpenCode | Agentic (CLI harness) | Each agent CLI's own auth via secrets | Experimental; stale since 2025-09-17 |
| `icoretech/codex-action` (13★) | Runs Codex CLI non-interactively via `codex-docker` container | Agentic (inside container) | `openai_api_key: ${{ secrets.OPENAI_API_KEY }}` or OAuth `codex_config` | Yes — pushed 2026-10-10 |
| `antongulin/robin` (39★) | BYOK PR review, summary + inline comments, optional agent mode (repo-wide analysis with tool calling) | Both (agent mode optional) | Secrets `LLM_API_KEY`, `LLM_BASE_URL`, `LLM_MODEL` — OpenRouter free tier works | Yes — pushed 2026-10-09 |
| `ai-action/ollama-action` (32★) / `ai-action/setup-ollama` (15★) | Run/pull Ollama models on Actions; `zakerby/gh-code-review` (20★), `ray5273/ollama-pr-review-action` (11★) review PRs with self-hosted Ollama | Mostly single-shot | No cloud key — self-hosted/localhost endpoint (key-free BYO-model); setup action eases install | Moderate — pushed 2026-06-24 |

Sources: per-repo `gh api` + raw READMEs on 2026-10-10 (linked repo names above); stars/pushed_at from the API responses.

Notable patterns from the table: (a) nearly every BYOK action passes the key as a GitHub Actions secret consumed by an input or env var; (b) official vendor actions (OpenAI, Anthropic) invest the most in key isolation (Responses API proxy, WIF federation); (c) provider-agnostic actions add a base-URL input (`appleboy/LLM-action`, `robin`, `anc95` via `OPENAI_API_ENDPOINT`); (d) gateway-based actions (OpenRouter) demonstrate BYOK across 100+ providers with one secret.

## 3. Self-built agent action write-ups (architecture patterns)

### 3.1 Cloudflare + Astro "software factory" — `withastro/triagebot-action`

Verified source: blog post "How we built a software factory to drive Astro's GitHub issue count to zero" (blog.cloudflare.com/astro-issue-triage/, fetched 2026-10-10) + `gh api` on `withastro/triagebot-action` (222★, pushed 2026-08-28, "A GitHub Action for issue triage"). Results claimed and stated there: open issues went from 200+ to ~30.

Architecture patterns worth copying:

- **Same harness local + CI**: the automation is an "agent skill" developed and tested locally with a coding harness, then the identical skill runs in a GitHub Action — total reuse (no CI-only logic).
- **Isolated subagents per phase**: Reproduce → Diagnose → Verify → Fix each run in a *separate isolated subagent*; state passes forward via a compiled `report.md` file. This counters "LLM bias toward forcing a solution" and limits each step's blast radius.
- **Stateless state machine over issue labels**: pipeline holds no state of its own; it derives position from issue labels (`triage needed` → `fix verified`) and re-reads existing comments. Any failure can be resumed by re-triggering.
- **Separate read and write identities**: the action takes `read-token: ${{ secrets.GITHUB_TOKEN }}` and a *different* `write-token: ${{ secrets.BOT_GITHUB_TOKEN }}`; the untrusted analysis stage only ever sees the read token. Additional BYOK secrets: `cloudflare-api-key`, `cloudflare-account-id`; model choice is provider-qualified strings (`triage-model: cloudflare-workers-ai/@cf/moonshotai/kimi-k2.7-code`).
- **Human verification before PR**: agents publish a *preview release* (via pkg.pr.new) and post instructions to the issue; only after the original reporter confirms does the pipeline open a PR. Human-in-the-loop on the reporter's own machine, not on the runner.
- **Decoupled, testable action repo**: triage logic was moved out of the Astro monorepo into a standalone repo so it can be tested and versioned independently (`uses: withastro/triagebot-action@v1`).
- **Failure = codebase signal**: when agents repeatedly fail on one area, treat it as missing tests/docs/opaque abstractions and fix the repo, not the prompt.
- Generalized engine became **Flue**, an open platform-agnostic durable-agent framework ("reacting to an event, running isolated subagents, separating reasoning from allowed actions is just a workflow").

### 3.2 OpenAI — `openai/codex-action` (vendor self-build, deeply documented)

Verified sources: README + `docs/security.md` in `openai/codex-action` fetched 2026-10-10.

- **Action container wraps the agent CLI**: installs a pinned `@openai/codex` version and runs `codex exec` non-interactively.
- **Key never reaches the agent process**: the action "configures it with a secure proxy to the Responses API" — Codex talks to a localhost proxy; `OPENAI_API_KEY` lives in the proxy, so the sandboxed agent cannot exfiltrate it directly (README: "Users must provide an API key ... as a GitHub Actions secret to use this action").
- **Runner-level privilege reduction ("safety-strategy")**: default `drop-sudo` revokes sudo, strips capabilities/`no_new_privs`, and removes runner access to root-owned service sockets (incl. the Docker socket) before Codex starts; `unprivileged-user` runs Codex as a dedicated low-priv account; `read-only` sandbox; `unsafe` only for fully trusted prompts.
- **Agent-level permission profiles**: `permission-profile: ":workspace"` / `:read-only` (beta) independently constrain filesystem and network; profiles do not compose with legacy `--sandbox`; `--full-auto` rejected with a profile.
- **Trigger allowlisting**: by default only users with write access to the repo can trigger; `allow-users`, `allow-bots` (github-actions[bot]), `allow-bot-users` extend explicitly, and docs warn not to rely on manual labeling alone for untrusted content.
- **Prompt-injection surface enumerated** in `docs/security.md`: PR bodies (HTML comments), commit messages, repo instruction files (`AGENTS.md`, `AGENTS.override.md`), screenshots/media — all treated as untrusted input when operating on PR-controlled content.
- **Shell hygiene**: pass untrusted values through `env:` and quote them; never splice `${{ ... }}` into `run:` scripts.
- Their own example workflow shows a **two-job split**: unprivileged `codex` job (contents: read) outputs `final-message`; a second job with `issues: write` posts the comment.

### 3.3 Anthropic — `anthropics/claude-code-action` (vendor self-build)

Verified sources: README, `docs/setup.md`, `docs/security.md` fetched 2026-10-10.

- **Identity = GitHub App, not PAT**: workflow uses the Claude GitHub App (or a custom app you create via manifest) minting short-lived, repo-scoped installation tokens; `docs/security.md`: "The GitHub app receives only a short-lived token scoped specifically to the repository", "No Cross-Repository Access".
- **Trigger gating**: only users with write access can invoke it (checked for issue/PR/comment/review and `workflow_run` actors); bots blocked by default; `allowed_non_write_users` is explicitly flagged RISKY and must pair with the auto-scoped `GITHUB_TOKEN`, never a static PAT ("a static token does not rotate between runs and could be partially or fully recovered over time via prompt injection").
- **BYOK with three tiers**: static `ANTHROPIC_API_KEY` secret, `CLAUDE_CODE_OAUTH_TOKEN` (subscription, minted by `claude setup-token`), or **Workload Identity Federation** — exchange the workflow's OIDC token (`id-token: write`) for a short-lived Anthropic token bound to `repo:owner/repo:` claims, "no secret to create, store, or rotate".
- **Prompt-injection sanitization**: strips HTML comments, invisible characters, markdown image alt text, hidden HTML attributes and HTML entities from mention content — with the caveat that new bypasses may emerge.
- **Human-in-the-loop PR creation**: by default Claude commits to a branch and *links to the PR-creation page*; "The user must click the link and create the PR themselves, ensuring human oversight".
- **Commit provenance**: optional commit signing via GitHub API signing or an SSH signing key secret.
- **Dangerous-event guidance**: explicit section on `pull_request_target`/`workflow_run` — "Do not check out an untrusted ref into the workspace root before this action" (base-ref checkout at root; PR head into a subdirectory via `--add-dir`).

### 3.4 Community self-builds showing further patterns

- **`Lixiaoyiao/deepseek-harness-action`** (verified README 2026-10-10): "The Action starts a credential-isolated DSH worker, validates its structured result, and lets a trusted Controller publish comments or validated changes" — the two-process split (untrusted worker without credentials / trusted publisher with a token) generalizes the OpenAI proxy idea to *any* agent, and the Controller validates a *structured* result schema before publishing.
- **`aeonfun/aeon`** (verified README 2026-10-10): an org-less "anyone can fork" pattern — "Keys go straight into your repo's encrypted secrets; Aeon Connect only hosts the dashboard, and every run happens on your own GitHub Actions"; one GitHub App drives instances "with no PATs or LLM billing" on the framework side; capability tiers (read-only skills), irreversible actions fail closed, optional per-run auth gate, secrets kept off the command line.
- **`icoretech/codex-action`**: container isolation — the agent CLI runs inside a Docker image (`codex-docker`) instead of directly on the runner; auth is exactly one of `openai_api_key` secret or an OAuth `codex_config`, and the action fails if both or neither are set (clean fail-closed input validation).
- **Bitrise "Kolega"** (bitrise.io blog "How we built a coding agent that lives in Slack, and the recipe to build your own", URL verified 200 on 2026-10-10; title verified): an agent harness that delegates work to disposable environments — the same "small trusted orchestrator, disposable untrusted executor" shape as the above.

## 4. BYOK security checklist for GitHub Actions

Each item verified against the cited GitHub official doc on 2026-10-10. These are the rules our action's workflows and docs must satisfy (and enforce where we can).

**Fork PRs and event trust**

1. Default to the `pull_request` event for untrusted contributions: on PRs from forks it grants a read-only `GITHUB_TOKEN`, withholds all other secrets, and runs *the PR's* workflow file (merge commit). Source: https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target
2. `pull_request_target` is elevated trust — base-repo read/write `GITHUB_TOKEN` **and** access to repo/org secrets, with workflow code taken from the base branch. Never combine it with checkout-and-execute of PR head code ("pwn request", root cause of multiple supply-chain compromises). Sources: https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target and https://securitylab.github.com/research/github-actions-preventing-pwn-requests/
3. GitHub is rolling out a **default event policy blocking `pull_request_target` in public repositories** (currently evaluate mode; enforcement date 2026-11-02) — design so we never need that event. Source: https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target
4. `actions/checkout` now blocks unsafe checkout of fork PR head refs in privileged contexts; the opt-out input is literally `allow-unsafe-pr-checkout: true` (named to be greppable). Never set it. Source: https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target
5. Fork-triggered runs get no secrets at all: "With the exception of GITHUB_TOKEN, secrets are not passed to the runner when a workflow is triggered from a forked repository." Plan the agent's unauthenticated (or environment-gated) behavior for fork PRs. Source: https://docs.github.com/en/actions/security-for-github-actions/security-guides/using-secrets-in-github-actions
6. `pull_request_target` workflows get read-only cache access to prevent cache poisoning; don't opt out via a write-capable `cache-mode`. Source: https://docs.github.com/en/actions/reference/security/securely-using-pull_request_target

**Script and prompt injection**

7. Never interpolate untrusted event fields (`github.event.pull_request.title`, issue bodies, branch names) into `run:` scripts; pass through `env:` and double-quote, or use an action input. Source: https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions
8. Treat PR titles/bodies, commit messages, repo instruction files (`AGENTS.md`), and attached media as untrusted prompt-infection vectors (sanitizing helps; assume bypasses exist). Sources: https://github.com/openai/codex-action/blob/main/docs/security.md and https://github.com/anthropics/claude-code-action security doc.

**Secrets handling**

9. Provider keys live only in GitHub Actions secrets, consumed via `secrets.*` into `env:`/`with:`; never as command-line arguments ("avoid passing secrets between processes from the command line ... use environment variables, STDIN"). Sources: https://docs.github.com/en/actions/security-for-github-actions/security-guides/using-secrets-in-github-actions
10. Any secret-derived or transformed value (JWT, base64) must itself be registered/masked or redaction fails; never store structured blobs (JSON/YAML) as one secret. Use `::add-mask::` for sensitive non-secrets. Source: https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions
11. Rotate provider keys; audit which secrets workflows actually use; delete stale ones. Source: https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions
12. Secret scanning safety net: GitHub's supported patterns include **OpenAI API key** (`openai_api_key`) and **Anthropic API key** (`anthropic_api_key`, plus admin/service-account keys) with push protection — enable secret scanning + push protection on our repos so leaked keys are blocked at push time. Source: https://docs.github.com/en/code-security/reference/secret-security/supported-secret-scanning-patterns

**Tokens and identity**

13. Least privilege for `GITHUB_TOKEN`: set restrictive default `permissions:` and expand per-job; the official token-authentication doc states you should "grant the GITHUB_TOKEN the least required access" via the `permissions` key. Sources: https://docs.github.com/en/actions/security-for-github-actions/security-guides/automatic-token-authentication and security-hardening guide.
14. Prefer a **GitHub App installation token** over a PAT for any write the agent performs (scoped to one repo, short-lived, revocable); mint it in-workflow with the official `actions/create-github-app-token` action (verified: 897★, official `actions` org, pushed 2026-10-06). Sources: https://github.com/actions/create-github-app-token and https://github.com/anthropics/claude-code-action security doc ("short-lived token scoped specifically to the repository").
15. Gate privileged jobs behind **environments with protection rules**: required reviewers (up to 6, with "Prevent self-review"), wait timers, branch/tag restrictions — a job cannot access environment secrets until a reviewer approves. This is the sanctioned human-approval gate for agent runs on untrusted input. Source: https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/manage-environments

**Runner isolation and job hardening**

16. Run untrusted agent work on **GitHub-hosted runners** ("ephemeral and clean isolated virtual machines"); "self-hosted runners should almost never be used for public repositories" because they can be persistently compromised by workflow code — which would put the provider API key at risk. Source: https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions
17. Add **runner-level privilege reduction** before starting the agent (the `openai/codex-action` `drop-sudo` model: revoke sudo, drop capabilities, `no_new_privs`, remove access to the Docker socket), or run the agent in a container. Source: https://github.com/openai/codex-action (Safety Strategy section).
18. Bound every job/step: `timeout-minutes` (step-level max 360, must be a positive integer) and `concurrency` groups with `cancel-in-progress` to prevent runaway agent loops burning minutes and tokens. Sources: https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions and https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/control-the-concurrency-of-workflows-and-jobs
19. Pin all third-party actions to full-length commit SHAs (the only immutable release form); enable Dependabot alerts for actions and CodeQL GitHub-Actions scanning. Source: https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions
20. Restrict who can trigger the agent (write-access check on issue/PR/comment actors — as both `claude-code-action` and `codex-action` do), and make bot triggering opt-in per trusted bot. Sources: https://github.com/anthropics/claude-code-action (docs/security.md) and https://github.com/openai/codex-action/blob/main/docs/security.md
21. Consider disabling "GitHub Actions creating/approving PRs" repo-wide and using branch protection on agent-pushed branches. Source: https://docs.github.com/en/actions/security-for-github-actions/security-guides/security-hardening-for-github-actions

## 5. Provider-agnostic model access patterns

The de-facto interoperability contract in every BYOK action surveyed is the **OpenAI-compatible endpoint**: `base_url` + `api_key` + `model`.

- **The convention**: any provider that serves `/v1/chat/completions` (and now `/v1/responses`) works with OpenAI SDKs by swapping `base_url`. Verified example from OpenRouter docs: `client = OpenAI(base_url="https://openrouter.ai/api/v1", api_key="<OPENROUTER_API_KEY>")` against `/api/v1/chat/completions`. Source: https://openrouter.ai/docs
- **Actions that implement it**: `appleboy/LLM-action` (one action for "all OpenAI-compatible services (OpenAI, Azure, Ollama, LocalAI, vLLM)" plus custom CA/headers for air-gapped endpoints — its README explicitly frames this as anti vendor lock-in); `anc95/ChatGPT-CodeReview` (`OPENAI_API_ENDPOINT` + `MODEL` inputs); `antongulin/robin` (`LLM_BASE_URL` + `LLM_MODEL` + `LLM_API_KEY` — OpenRouter free tier works); `i18n-actions/ai-i18n` (Anthropic, OpenAI, Ollama, Bedrock). Sources: the repos' READMEs (2026-10-10).
- **Model gateways**: OpenRouter is the leading CI-friendly gateway — one BYOK secret, provider-qualified model strings (`owner/model`), free-tier models usable, and docs sections for **BYOK** (users can also attach their own provider keys) and **Spend Controls** ("Spend Controls - Budget and Guardrail Best Practices", the cost-cap layer GitHub Actions cannot provide natively). Source: https://openrouter.ai/docs and https://openrouter.ai/docs/guides/best-practices/spend-controls
- **Self-hosted/localhost endpoints**: Ollama actions (`ai-action/ollama-action`, `zakerby/gh-code-review`, `ray5273/ollama-pr-review-action`) show the key-free BYO-model end of the spectrum; a provider-neutral action should accept a base URL so these fit without code changes. Source: repos' READMEs (2026-10-10).
- **Auth-escalation ladder** (Anthropic pattern, worth supporting): static API key secret → OAuth/subscription token → OIDC Workload Identity Federation (no stored secret; short-lived token bound to repo claims). Source: https://github.com/anthropics/claude-code-action docs/setup.md.
- **What a BYOK action should therefore expose**: `api_key` (secret), optional `base_url`, `model` (free-form, provider-qualified), optional `api_version`/extra headers for Azure-style providers, and a socket for gateway spend caps; keep the provider layer behind one trait so new providers are config, not code.

## 6. Recommended secret/identity architecture for our project

Synthesis of the verified patterns above, as concrete design rules for the storytold agent action:

1. **Split identities, minimum privilege** (Astro `triagebot-action` pattern): the analysis phase gets only the auto-scoped `GITHUB_TOKEN` with `contents: read`; any write (comments, commits, PR creation) is done by a *second job* or trusted controller step holding a **GitHub App installation token** minted by `actions/create-github-app-token`. Never a fine-grained PAT in workflow files. The agent core never holds a GitHub token with more than the permissions its current task needs.
2. **Keep the model key away from agent-controlled processes** (OpenAI proxy pattern): pass the BYOK key to a thin broker (subprocess or localhost proxy) that the agent calls; the agent sandbox gets a bearer handle, not the raw secret. If we cannot ship a broker in v1, at minimum: `env:`-only, never `run:` args, never logged, masked if ever surfaced; never expose it in outputs (`show_full_output`-style options default off).
3. **Events**: listen on `pull_request` (fork-safe: read-only token, no secrets) + `issue_comment`/`issues`/`workflow_dispatch` gated by a write-access check; on fork PRs run in read-only "comment-only" mode or require a maintainer label + environment approval to unlock key access (fork runs get no secrets anyway — design that degraded path deliberately). Never use `pull_request_target`; document the GitHub default block policy as our default too.
4. **Human-in-the-loop for state changes** (Claude pattern): agent pushes a branch and posts a create-PR link (or opens a draft PR); merging requires normal branch protection; optionally gate the write job behind an **environment with required reviewers** for untrusted-authored triggers. Reproduce → Diagnose → Verify phases run as isolated subagents handing off structured reports (Astro pattern), with verification = our own cargo test/clippy/rustfmt runs on the ephemeral runner.
5. **Sandboxing**: run the agent under the OpenAI-style safety ladder — default `drop-sudo`-equivalent + workspace-write profile (or a container), network deny-list except the model endpoint, `timeout-minutes` on every agent step, `concurrency` group per PR/issue with `cancel-in-progress: true`.
6. **Cost control**: per-job model call/token budget enforced in our broker plus provider-side spend caps (OpenRouter spend controls / provider budget limits); document both.
7. **Sanitize and delimit untrusted content** before it enters prompts (strip hidden HTML/comments/invisible chars like `claude-code-action` does), and treat `AGENTS.md` from PRs as untrusted.
8. **Ship the security contract**: our README ships hardened example workflows (SHA-pinned actions, minimal `permissions:`, environment-gated secrets, secrets-only key handling), and our repo enables secret scanning + push protection (OpenAI/Anthropic key patterns are covered), CodeQL Actions scanning, and Dependabot for actions.
9. **Provider-neutrality as a feature**: inputs `api_key`/`base_url`/`model` defaulting to OpenAI-compatible; verified against OpenAI, any OpenAI-compatible host (Ollama/vLLM/LocalAI), Azure OpenAI (endpoint override), and OpenRouter as the one-secret many-model gateway.
10. **Resumability**: derive state from issue/PR labels and comments (Astro state-machine pattern) instead of external storage, so retries are free and any human can audit exactly what the agent did and why.
