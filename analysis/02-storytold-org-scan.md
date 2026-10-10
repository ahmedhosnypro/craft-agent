# Storytold org scan — input for the org-wide agentic GitHub Action
Task: ex2-storytold-org-scan. Date: 2026-10-10. Every claim cites the `gh` command (run today) that produced it.

## 1. Org overview
- `gh api orgs/storytold --jq '{login, type, created_at, public_repos}'` →
  `{"created_at":"2021-01-03T07:27:05Z","login":"storytold","public_repos":44,"type":"Organization"}`.
  44 public repos today (mission brief said 42; re-verified 44 via `gh repo list storytold --limit 100`).
- Identity: the **ArtCraft / Craft App Suite** — clean-room, pure-Rust re-implementations of commercial
  creative apps — plus an AI art product line and shared infra. Suite membership confirmed by
  craft-launcher README: PhotoCraft, LightCraft, FilmCraft, DesignCraft, VectorCraft, PdfCraft,
  EffectCraft, WordCraft, DeckCraft, GridCraft, CADCraft, SoundCraft (`gh api repos/storytold/craft-launcher/readme`).
- Maintainer: `echelon` = **Brandon Thomas**, company "ArtCraft", blog http://brand.io
  (`gh api users/echelon --jq '{login,type,name,company,blog}'` → `{"blog":"http://brand.io","company":"ArtCraft","login":"echelon","name":"Brandon Thomas","type":"User"}`).
  Human (type User), not a bot. Authored deckcraft PR #1 ("Rename PrintCraft to PdfCraft") and artcraft
  release PRs 0.39–0.42; commit trailers show `<echelon@gmail.com>` and `<shared2@storyteller.ai>`
  (filmcraft commit messages).
- No `.github` repo: `gh api repos/storytold/.github` → 404. No org-level workflow templates or
  community health files.
- No public `craftrules` repo: `gh api repos/storytold/craftrules` → 404 — yet AGENTS.md/CLAUDE.md in
  filmcraft/pdfcraft/lightcraft/cadcraft/vectorcraft link
  `https://github.com/storytold/craftrules/blob/main/standards/*.md` (never-crash, fonts, contributors)
  as THE cross-app standard, and commit messages say "craftrules is two levels up (../../craftrules)".
  Referenced as a sibling checkout — private or not-yet-public (see §7).
- Surface: getartcraft.com, discord.gg/artcraft, YouTube/@OfficialArtCraftStudios, X/@get_artcraft
  (artcraftx README). Releases: Windows MSI, macOS universal signed+notarized DMG, Linux
  AppImage/deb/rpm/tar/Flatpak, FreeBSD tarballs, web/WASM (wordcraft release.yml jobs:
  version/macos/windows/linux/flatpak/web/freebsd/release).
- Transfers: `gh api repos/echelon/craft-libs` resolves to `storytold/craft-libs` (`fork:false`) —
  repos were moved from Brandon's personal account into the org (API redirect).

## 2. Repo map
Stars/forks via `gh api repos/storytold/<r>` (2026-10-10); workflows-on-main verified with
`git/trees/main?recursive=1`. The `actions/workflows` API also lists workflows that exist only on
open PR branches (e.g. `ci`/`riscv64` for repos whose main has no ci.yml — see §3).

Active app repos (all pure Rust, MIT OR Apache-2.0, egui desktop + web/WASM, pushed Oct 2026):

| Repo | Clone of | Stars / forks | Workflows on main (verified) |
|---|---|---|---|
| photocraft | Photoshop | 39,411 / 5,750 | ci, docs, freebsd, packaging-lint, perf-nightly, release, windows-arm64 |
| artcraft | AI art studio (Tauri) | 13,153 / 2,036 | artcraft-windows-publish, artcraft-macos-publish (+dependabot dynamic) |
| lightcraft | Lightroom | 8,467 / 2,586 | freebsd, packaging-lint, release, windows-arm64 (+brand, orf-corpus in API) |
| filmcraft | Premiere Pro | 7,967 / 2,827 | freebsd, packaging-lint, release, windows-arm64 |
| pdfcraft | Acrobat | 7,338 / 2,549 | ci, freebsd, packaging-lint, release, windows-arm64 (+rust, windows-msi, Copilot dynamic) |
| vectorcraft | Illustrator | 5,728 / 2,333 | affinity-corpus, freebsd, packaging-lint, release, windows-arm64, windows7 |
| effectcraft | After Effects-style | 4,267 / 1,864 | freebsd, gl-debug, packaging-lint, release, windows-arm64 |
| wordcraft | Word | 2,637 / 764 | freebsd, packaging-lint, release, windows-arm64 |
| designcraft | design tool | 2,380 / 1,380 | freebsd, packaging-lint, release, windows-arm64 |
| gridcraft | Excel | 1,546 / 583 | docker, freebsd, packaging-lint, release, windows-arm64 |
| cadcraft | AutoCAD | 1,621 / 664 | packaging-lint, release |
| soundcraft | Pro Tools | 1,171 / 612 | freebsd, packaging-lint, release, windows-arm64 |
| deckcraft | PowerPoint | 1,211 / 547 | freebsd, packaging-lint, release |
| craft-launcher | suite installer/updater | 206 / 81 | ci, freebsd, packaging-lint, release, windows-arm64 |

Notes: effectcraft/designcraft have empty repo descriptions (`gh repo list`); targets inferred from
suite context. photocraft's workflow API also lists android, chatgpt, flatpak-test, fxc-probe,
history-cache, homebrew, local-models, native-packaging, nix, winflaky-probe — registered from
non-main branches, not on main (verified tree).

Shared-infra repos:

| Repo | Purpose | Stars / forks | CI |
|---|---|---|---|
| artcraft-services | Rust/Actix backend `storyteller-web` (api.storyteller.ai), MySQL/sqlx, workers, Stripe, Nx/React frontend | 563 / 524 | docker-publish-cpu only; ci.yml parked in `.github/workflows_inactive/` |
| artcraftx | ArtCraft-X: minimal Tauri desktop for AI creation (images/video/audio/3D/worlds) | 460 / 340 | none |
| craft-fonts | Shared fonts: manifest + SHA-256, OFL fonts, Rust `craft-fonts` crate (+embed feature) | 185 / 218 | ci (fmt, clippy -D warnings, test --locked, embed test) |
| photocraft-corpus | JS tooling + 258 scripted Photoshop PSDs as test oracles | 70 / 94 | none |
| craft-repo-status-app | Tauri 2 tray dashboard polling org repos | 28 / 38 | none |
| craft-libs | New shared-Rust-libs workspace; empty scaffold (`crates/.gitkeep`) | 36 / 55 | none |
| cloud-worker | Python ComfyUI/RunPod ops for MiniMax H3 video model | 27 / 52 | none |
| github-media | README videos/gifs (mp4/webp/gif) | 16 / 35 | none |

Older/legacy (2020–2025, not agent targets): spark (TS gaussian splatting, fork),
placeholder-artcraft (archived), FineTrainers-Conditioning, storyteller-ml, irsa-manager,
storyteller-bevy, bevy-mocap, terraform-aws-observability-accelerator, html_test, vits-finetuning,
xsens-packet-send, UE4 C++ plugins (EventCenterPlugin, ActorSpawningPlugin, RuntimeAudioImporter,
LiveScan3D, RuntimeFilesDownloader, UnrealZeroMQ), k4a-sys-temp, point-generator,
Azure-Kinect-Sample-for-Unity, UE4-OSC, realtime-voice-conversion (`gh repo list`, 2026-10-10).

## 3. CI gate pattern across app repos
Method: workflow files fetched with `gh api repos/storytold/<r>/contents/.github/workflows/<f>`
(filmcraft, wordcraft, photocraft, pdfcraft, craft-fonts); registry via `actions/workflows`;
files-on-main via `git/trees/main?recursive=1`.

1. **ci.yml — the Rust gate.** Merged on main only in photocraft and pdfcraft. wordcraft, filmcraft,
   gridcraft, vectorcraft, lightcraft, soundcraft, effectcraft, designcraft, deckcraft have NO ci.yml
   on main today; it arrives via open fork PRs (item 6). Contents (photocraft + pdfcraft files, near
   identical):
   - `cargo fmt --all -- --check`
   - `cargo test --workspace --locked` (+ variants: without optional features e.g. `heif`; with
     `CRAFT_FONTS_DIR` at a pinned `storytold/craft-fonts` checkout, `CRAFT_FONTS_REQUIRED=1`)
   - `cargo clippy --workspace --all-targets --locked -- -D warnings`
   - `cargo xtask` checks: `layers` (dependency layering), `scorecard --check`, `corpus --all` +
     `test-corpus` (sha256-pinned corpora), `wasm` (wasm32-safe crate check)
   - Platform-selection job: ubuntu + windows on main or `full-ci` label; Linux-only on PRs unless
     UI/platform files change. **No macOS on PRs or main — "the org's 5 macOS slots are kept for
     release builds"** (comment in photocraft and pdfcraft ci.yml)
   - Third-party actions pinned by commit SHA (filmcraft windows-arm64.yml pins
     Swatinem/rust-cache@6323deb…; commit "Windows ARM64 workflow: pin third-party actions by commit")
2. **packaging-lint.yml** (every app repo; filmcraft file fetched): actionlint, shellcheck, PowerShell
   parser, xmllint (WiX/plist/XML), desktop-file-validate, appstreamcli, Flatpak src-vs-bundle
   manifest consistency, FreeBSD package dry-run. Seconds, no Rust build.
3. **windows-arm64.yml**: cross-compile MSI on x64 → install/verify/run CLI on a `windows-11-arm`
   runner (filmcraft file).
4. **freebsd.yml**: nightly FreeBSD 14.3 VM build+package on a Linux runner (cron `37 2 * * *`),
   PRs only when packaging/freebsd files change (filmcraft file).
5. **release.yml**: push to `release` branch / dispatch; `environment: release` guards signing
   secrets (APPLE_CERTIFICATE, APPLE_ID…); `macos-15-xlarge` (M2 Pro) larger runner with monthly
   spend cap on the `actions_macos_xl` SKU; jobs version/macos/windows/linux/flatpak/web/freebsd/
   release (wordcraft file). Packaging + signing only — **no cargo test here**.
6. **Pending org-wide CI PRs**: dylanmtaylor's fork PRs "ci: Add RISC-V Linux build" (wordcraft #131,
   filmcraft #502, gridcraft #90, deckcraft #51 — all open; runs show `action_required`, the
   new-workflow-approval state). That is why `ci`/`RISC-V` workflows appear in the API for repos
   whose main has no ci.yml.
7. **Local gate discipline** covers repos without merged ci.yml: filmcraft CLAUDE.md requires before
   every commit `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `cargo xtask layers/assets/wasm`, all wrapped by `cargo xtask ci`.
   PR bodies state which gates ran (wordcraft #60: "cargo fmt --check and cargo xtask layers pass",
   "cargo xtask ci not yet run end to end").
8. **craft-fonts ci.yml** is the minimal form: fmt, clippy --all-targets --all-features --locked
   -D warnings, test --workspace --locked, plus `-p craft-fonts --features embed`.

## 4. Shared-infra / tooling repos deep-dive
- **craft-repo-status-app** — Tauri 2 tray app ("Craft Status") polling storytold `*craft` repos every
  5 minutes via GitHub GraphQL/REST; everything local, no server (README). Rust workspace:
  `craft-core` (model, release/download accounting, urgency heuristic, config, cache — no network/UI
  deps, unit-tested), `craft-github` (GraphQL/REST fetcher + `craft-fetch` debug CLI),
  `craft-status-app` (Tauri shell: poll thread, tray, global shortcut). UI: plain HTML/CSS/ES
  modules, no bundler, tested with `node --test`. Urgency heuristic (`crates/craft-core/src/
  urgency.rs`): crash/hang/data-loss/security +40, broken-but-not-severe +15, `bug` label +15,
  filed by non-team +10, reactions×3 + comments×2 log-scaled up to +40, names latest version +10,
  opened <24h +8 / <72h +4, feature-request/question/docs −25. This is the org's existing
  "what needs attention first" model — an agent's triage should align with it.
- **craft-libs** — brand-new (pushed 2026-10-09) shared Rust workspace: `resolver = "3"`, edition
  2024, rust-version 1.90, workspace lints `unsafe_code = "forbid"`, MIT OR Apache-2.0; `crates/`
  holds only `.gitkeep` — no libraries yet. Apps will consume by pinned rev:
  `craft-<name> = { git = "https://github.com/echelon/craft-libs", rev = "<commit>" }` (README).
  AGENTS.md: "No app code"; each crate denies clippy unwrap/expect/panic/todo/unreachable; merge
  gates fmt/clippy(-D warnings)/test. Standards delegated to craftrules. No CI.
- **craft-fonts** — the font store: `fonts/manifest.txt` (family/style/file/scripts/licence/SHA-256),
  ATTRIBUTION.md, `crates/craft-fonts` (manifest as static table, CRAFT_FONTS_DIR discovery, `embed`
  feature) and `crates/craft-fonts-tests`; apps take it as a fully optional build input, never a Cargo
  dependency (README). App CI pins a commit: `repository: storytold/craft-fonts, ref: abb8331…`
  (photocraft/pdfcraft ci.yml, wordcraft release.yml). Houses BIZ UDPGothic, Shippori Mincho, BIZ
  UDMincho, Noto Sans CJK SC, Noto Sans Arabic — all OFL.
- **artcraft-services** — ArtCraft backend: `storyteller-web` Rust/Actix/Tokio HTTP service
  (api.storyteller.ai), `artcraft_router` provider clients, async workers, MySQL via sqlx
  (compile-time queries isolated in a `mysql_queries` crate), object storage, Stripe billing; Nx/React
  frontend (README + tree). `.claude/rules/` holds api-conventions.md, code-style.md, testing.md —
  e.g. two-space indent, log macros not println!, `maybe_` prefix, sqlx only in mysql_queries.
  `.github/dependabot.yml` present; only docker-publish-cpu.yml active (15–25 min GHCR builds,
  path-filtered); ci.yml + gpu docker workflows parked in `.github/workflows_inactive/`. A `.env`
  file is tracked in the public tree (seen in `git/trees/main` listing; not read).
- **cloud-worker** — Python ops for the **MiniMax H3** open-weights video model on RunPod GPU pods
  (1× B200 rig) via ComfyUI: idempotent pod install/weights scripts, local push/port-forward
  helpers, benchmark harness (t2v/i2v/r2v × duration × resolution × weight family → CSV/JSONL), and
  docs: WEIGHTS, RESEARCH (SGLang/vLLM-Omni/diffusers options), BENCHMARKS, CONCURRENCY
  (cost-per-video math), MITIGATIONS (OOM maps), SERVING-PLAN (paid/free pool design) (README).
  This is the AI-generation backend ArtCraft rides on; last push 2026-08-07.
- **artcraftx** — "A minimal desktop app for AI creation" — images, video, audio, 3D meshes, worlds
  (README). Rust + Tauri + sqlx workspace with `Cranky.toml` and per-crate CLAUDE.md files
  (`git/trees/main`). "Early Experimental Release — builds forthcoming." Last push 2026-09-27.
- **craft-launcher** — "One place to install, update and open every app in the Craft App Suite,
  rebuilt in pure Rust" (README); carries the full app-repo CI set including ci.yml.
- **github-media** — raw mp4/webp/gif media for READMEs; workflow is upload-via-issues + copy URL;
  no CI (README). Last push 2026-07-16.

## 5. Bot / AI activity found
- **GitHub App bots with PR activity:**
  - `Copilot` (Bot) — **pdfcraft only**. Registered workflow `dynamic/agents/copilot-pull-request-reviewer`
    (`gh api repos/storytold/pdfcraft/actions/workflows`); review comments on `crates/sign/pdf.rs`,
    `crates/sign/pkcs12.rs`, `crates/sign/x509.rs`, `crates/automation/src/signing.rs`
    (`gh api repos/storytold/pdfcraft/pulls/comments` filtered to type Bot).
  - `app/dependabot` (Bot) — **artcraft only**: Nx/@nx/React, tailwind, postcss, dompurify, axios,
    adm-zip etc. bumps in `/frontend` (artcraft PRs #2044–#2049, #1966, #1968, #1960); workflow
    `dynamic/dependabot/dependabot-updates` registered.
  - No `jules` or `renovate` PRs anywhere (`gh search prs --owner storytold jules/renovate`; PR lists
    for filmcraft, photocraft, wordcraft, gridcraft, deckcraft, artcraft). `gh api users/jules` is a
    User anyway.
  - Zero bot-authored PRs in filmcraft/photocraft/vectorcraft/lightcraft/wordcraft (recent 30 per
    repo, `user.type != "User"` filter empty).
- **AI co-authorship in commits — the dominant automation signal.** GitHub commit search:
  `"Co-Authored-By: Claude" org:storytold` → **total_count 4,984**; `"Co-authored-by: Codex"` → **89**;
  `"agent@localhost"` → 1 (`gh api -X GET search/commits -f q=... --jq .total_count`). Per-repo Claude
  trailer counts: lightcraft 950, vectorcraft 907, photocraft 722, effectcraft 661, filmcraft 607,
  pdfcraft 460, designcraft 327, soundcraft 91, wordcraft 87, deckcraft 44, cadcraft 26, gridcraft
  24, artcraft 2. Trailers seen: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (sometimes
  "(1M context)"), `Claude-Session: https://claude.ai/code/session_...` (pdfcraft #264),
  `Co-authored-by: Codex <noreply@openai.com>` (wordcraft #91, pdfcraft #305), and
  `Co-authored-by: WordCraft agent <agent@localhost>` (wordcraft #60 — a contributor's local agent).
  Both the maintainer and outside contributors work this way; merges often add Claude trailers to
  human PRs (e.g. photocraft #611, pdfcraft #310).
- **The apps are agent-first by design.** filmcraft docs/agents.md: "FilmCraft is built to be driven
  by AI agents, and largely built by them." Each app ships a CLI with an MCP server (`filmcraft-cli
  mcp`, headless `--demo` and `--bridge` modes to the running desktop app), a JSON-lines control
  channel (`filmcraft --control 9876`), automation ids on every widget, `command_list`/
  `command_run`/`command_batch` tools, and `.mcp.json` registering servers for Claude Code. UI work
  is verified by driving the app and taking `ui.screenshot`.
- **Agent-governance files are standard in every app repo:** AGENTS.md (rules that "must never be
  broken", wins over CLAUDE.md), CLAUDE.md (working instructions: read plan/STATUS.md first —
  maintainer-local gitignored —, follow the autonomous-operation protocol, one task id per commit,
  run gates before commit), docs/agents.md. craft-libs, artcraftx and artcraft-services carry the
  same pattern (AGENTS.md, per-crate CLAUDE.md, `.claude/rules/`).
- **Rules the org enforces on every contributor, human or AI** (filmcraft AGENTS.md): never crash
  (no unwrap/expect/panic/todo outside tests, clippy deny lints per crate), no `unsafe` (except one
  platform-FFI crate with SAFETY comments), treat every input-derived number as hostile, bounded
  recursion, `catch_unwind` workers; **assets**: never any Adobe artwork, every asset needs an
  `<file>.attribution` sidecar + ATTRIBUTION.md row (`cargo xtask assets` enforces), fonts only via
  craft-fonts; **clean-room**: implement from public specs, never read Adobe bundles or GPL code;
  permissive deps only; **credits**: contributors/contributors.json + people.toml with an About
  "Models" tab crediting AI models used (cadcraft #3, vectorcraft #411, photocraft #898, lightcraft
  #252 — all via the craftrules standard).
- **Human automation contributor:** `dylanmtaylor` (type User) runs an org-wide tooling pipeline via
  fork PRs: "ci: Add RISC-V Linux build" (open: wordcraft #131, filmcraft #502, gridcraft #90,
  deckcraft #51), "chore: bump github actions to fix Node 20 deprecation warnings" (merged: filmcraft
  #501, wordcraft #220, gridcraft #165, deckcraft #52, artcraft #2040), "Add semantic-release
  automation on main" (open: deckcraft #43, artcraft #2033), "Add OpenWiki automation and Pages
  publishing" (open: deckcraft #28 + pdfcraft, effectcraft, vectorcraft, photocraft, filmcraft,
  wordcraft, artcraft, cadcraft, designcraft, gridcraft variants), "Publish Flatpak repository
  through GitHub Pages" (open: deckcraft #48). Files: `.github/workflows/openwiki-*.yml`,
  `semantic-release.yml`, `.releaserc.json` (deckcraft PR file lists).

## 6. Implications for an org-wide agentic GitHub Action
1. **Follow the house naming/conventions.** Shared/tooling repos are `craft-*` (craft-launcher,
   craft-libs, craft-fonts, craft-repo-status-app) — name the agent repo `craft-<agent>-action` (or
   similar), dual-license MIT OR Apache-2.0, ship NOTICE + ATTRIBUTION.md, pure Rust with
   `unsafe_code = "forbid"`, and the AGENTS.md + CLAUDE.md pair (that pair is the org's de-facto
   agent contract, already in 6+ repos).
2. **There is no `.github` repo and no public craftrules** — no org-level templates or defaults
   exist. A new agent must be installable per-repo (workflow file) or as a GitHub App; creating
   `storytold/.github` (org profile + reusable workflow templates) is a natural companion and
   today nothing blocks it.
3. **It joins an existing human+AI pipeline.** ~5k Claude-trailer commits, contributor-side Codex and
   local agents, Copilot reviewing pdfcraft, dependabot on artcraft. The agent must: credit itself
   with a `Co-Authored-By: <model> <noreply@…>` trailer (established convention), avoid stepping on
   Copilot (review) and dependabot (deps) scopes, and expect `action_required` first-run approvals —
   the exact friction dylanmtaylor's pending workflow PRs sit in now.
4. **Review targets are already AI-authored.** PR bodies follow a house format (Summary / Clean
   room / Known gaps / Tests — wordcraft #60), so the agent can parse claims and verify them against
   the gates ("cargo xtask ci not yet run end to end" is a checkable gap).
5. **Machine-readable house rules to enforce in review:** AGENTS.md per repo + craftrules standards
   (never-crash, fonts, licensing, contributors). Concretely: no unwrap/expect/panic outside tests,
   no unsafe, checked math on input-derived numbers, asset sidecars (`cargo xtask assets`),
   clean-room provenance (spec citations, no Adobe assets, no GPL), fonts via craft-fonts build
   input only.
6. **Its own PRs must pass the real gates:** fmt --check, clippy --workspace --all-targets --locked
   -D warnings, test --workspace --locked, cargo xtask layers/assets/wasm (+ corpus/scorecard where
   present), packaging-lint (actionlint + shellcheck). Budget reality: Linux-only CI on PRs, no
   macOS (release-only, 5-slot budget), Windows on platform-touching PRs.
7. **Highest-value targets by traffic** (stars / open issues / PR churn, `gh api repos/<r>`):
   photocraft (39,411★, 762 open issues, 50-PR pages of churn), artcraft (13,153★, 116 issues,
   dependabot noise), lightcraft (8,467★, 230), filmcraft (7,967★, 234), pdfcraft (7,338★, 291),
   vectorcraft (5,728★, 81). Younger repos wordcraft (2,637★), gridcraft (1,546★), deckcraft
   (1,211★), cadcraft (1,621★), soundcraft (1,171★) have lower PR numbers — less review load, more
   room for the agent to raise consistency.
8. **BYOK has precedent but no LLM-in-CI usage yet.** Nothing in the org calls an LLM from Actions
   today (Copilot is GitHub-native; Claude/Codex work happens in local sessions). Secrets are
   environment-scoped like APPLE_* in `environment: release`; the agent's BYOK key should follow
   the same pattern (repo/org secret + gated environment). Note: artcraft-services tracks a `.env`
   in its public tree (not read) — the new repo should model better secret hygiene.
9. **Reuse before building:** craft-repo-status-app already contains a GraphQL org poller + urgency
   heuristic as Rust crates (`craft-github`, `craft-core`) — prime candidates to extract or depend
   on; craft-libs is the designated (still empty) home for shared crates the agent could seed.
   The MCP command model (command_list/command_run/command_batch, ui.set, screenshots) is the org's
   native agent surface for app-level work.

## 7. Open questions
1. Is `craftrules` a private repo or not yet created? Every app's standards link to it and every dev
   machine expects `../../craftrules`; it 404s publicly today. The agent's rulebook must live
   somewhere referenceable or its docs will rot.
2. Will the maintainer merge dylanmtaylor's pending ci.yml/RISC-V PRs (wordcraft #131, filmcraft
   #502, gridcraft #90, deckcraft #51), or does he want the agent to normalize CI across repos
   itself? All sit in `action_required` — first-run approval is a design constraint for any agent
   that adds workflow files.
3. Why is artcraft-services' ci.yml parked in `.github/workflows_inactive/` (cost? flakiness? Docker
   build as the only gate)? Determines whether the agent should review that repo's Rust at all.
4. Delivery shape: GitHub App (org-wide bot identity, comments/commits as bot, no per-repo files —
   the Copilot/dependabot pattern) vs reusable workflow per repo vs both (App + starter workflow)?
   No `.github` repo exists yet to host org templates.
5. LLM access: BYOK per-repo secret vs org-level secret vs GitHub-managed (Copilot billing)? No
   current CI usage to copy; the local-session pattern (Claude Code/Codex BYO accounts) is the only
   precedent.
6. Coordination with `dylanmtaylor`'s open automation PRs (semantic-release, OpenWiki Pages, Flatpak
   repo publishing) — merge order and ownership; his RISC-V PRs touch the same workflow files an
   agent would edit.
7. For repos with no ci.yml on main (wordcraft, filmcraft, gridcraft, deckcraft, …), gates run only
   locally: wordcraft #60 merged with a known clippy failure in wordcraft-engine ("a fix changes
   behavior"). Is local-gate-only intentional for the younger repos, or a gap the agent should flag
   in review?
8. ArtCraft apps credit AI models in an About "Models" tab (craftrules standards/contributors.md).
   Should the agent repo adopt the same contributor/model credit pipeline
   (contributors/people.toml + generated contributors.json)?

---
Verification trail (commands run today): `gh api orgs/storytold`; `gh repo list storytold --limit
100`; `gh api repos/storytold/<r>` and `.../actions/workflows` for all 22 active repos;
`git/trees/main?recursive=1` for wordcraft, photocraft, gridcraft, filmcraft, vectorcraft,
lightcraft, soundcraft, effectcraft, cadcraft, designcraft, pdfcraft, craft-libs, cloud-worker,
artcraftx, artcraft-services; workflow-file contents for filmcraft (packaging-lint,
windows-arm64, freebsd), wordcraft (release), photocraft (ci), pdfcraft (ci), craft-fonts (ci);
`gh pr list --state all --limit 50` for filmcraft, photocraft, wordcraft, gridcraft, deckcraft,
artcraft; `gh api users/<login>` for echelon, ahmedhosnypro, dylanmtaylor, MarekSpodBiedry,
MrCloudy2, jules, codex, dependabot; `gh api search/commits` for Claude/Codex/agent@localhost
trailer counts (org + per-repo); `gh api repos/<r>/pulls/comments` and `/issues/comments` for Bot
authors across 6 repos; `gh pr view`/`--json` for #501/#502/#618 (filmcraft), #228/#60 (wordcraft),
#793 (pdfcraft), #28/#43 (deckcraft); READMEs via `gh api repos/<r>/readme` for craft-libs,
craft-repo-status-app, cloud-worker, artcraft-services, craft-launcher, craft-fonts, artcraftx,
github-media, photocraft-corpus; AGENTS.md/CLAUDE.md/docs/agents.md via `contents/` for filmcraft,
craft-libs; `.claude/rules/code-style.md` for artcraft-services.
