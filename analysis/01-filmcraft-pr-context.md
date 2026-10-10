# FilmCraft PR #553 comment context — input for the storytold agentic GitHub Action

Task slug: `ex1-filmcraft-pr-context` · Date: 2026-10-10. Every claim below cites its
source (gh api / gh CLI output, local file paths, fetched URLs); nothing is guessed.

## 1. Comment & PR context

**The origin comment** — `ahmedhosnypro`, 2026-10-10T09:54:27Z, comment id 6096316169,
author_association CONTRIBUTOR, edited at 09:59:58Z. Verified text
(`gh api repos/storytold/filmcraft/issues/comments/6096316169`; live body has the typos
"consitency" and "suit"):

> "Thank you @echelon, noted, i will land these fixes, before moving forward,
> check the changes (while also we can figure out a more better solution, adding an
> experimental/beta/optional item to settings so users can manually enable it, though the
> photocraft app already has this behaviour, it would be proper to have consitency in the
> whole storytold suit):"

It carries two screenshot attachments (URLs recorded only — not viewed, per assignment):
- https://github.com/user-attachments/assets/39f906b6-e0c7-47b7-9cfd-1568ae7532a2 (alt "Screenshot_20261010_050543", 215x109)
- https://github.com/user-attachments/assets/089af401-92d6-4dc4-92ed-ce1aeff3d217 (alt "Screenshot_20261010_050538", 87x83)

**PR #553** — "linux: one top bar — the header is the window's title bar (PhotoCraft's
caption buttons)", author ahmedhosnypro (User type, account created 2016-12-08,
`gh api users/ahmedhosnypro`), OPEN, base `main` <- head `feat/linux-single-top-bar`,
+420/-6 across 7 files (`gh pr view 553 --json files`): `.gitignore` +1,
`apps/filmcraft/src/main.rs` +12, `crates/ui-egui/src/brand.rs` +27,
`crates/ui-egui/src/header.rs` +308, `crates/ui-egui/src/i18n/es.tsv` +2,
`crates/ui-egui/src/lib.rs` +64, `crates/ui-egui/src/theme.rs` +6.

The PR makes Linux windows undecorated with an in-app title bar copied from
`photocraft/crates/ui-egui/src/titlebar.rs` (same caption glyphs, 46 pt targets, hover
fills, edge-resize zones; `FILMCRAFT_DECORATIONS=1` env escape hatch), fixes #433, and
adds follow-up fixes: resize zones no longer steal caption clicks, header always kept
when `window_controls` is set, Wayland drops the Minimize button, `scratch/` gitignored.
PR body verification: 76 `filmcraft-ui-egui` lib tests + 47 integration targets pass,
clippy/rustfmt clean, live Wayland/X11 probes, pixel-adjudicated visual audit; and
explicitly: "the full `cargo xtask ci` workspace release gate was not run for this PR
(maintainer's call)" (`gh pr view 553 --repo storytold/filmcraft`).

**The thread** — `gh api "repos/storytold/filmcraft/issues/553/comments"`. There are no
formal PR reviews and no inline review comments (`pulls/553/reviews` and
`pulls/553/comments` both return empty); the "review" is a top-level issue comment:

1. id 6096129740, **echelon**, 09:29:01Z, author_association MEMBER — the review (§2).
2. id 6096316169, **ahmedhosnypro**, 09:54:27Z — the origin comment above.
3. id 6097015929, **ahmedhosnypro**, 11:27:33Z — proposes the agentic GitHub Action
   itself (§6); verbatim:
   > "@echelon  i would recommend either creating a parent repo that all suite projects
   > are its children, then creating a github cron actions that runs once or twice
   > daily, it will summerize merged pull requests, common patterns, things that are
   > common and need global adaption, duplicate code that can be moved to reusable
   > crates — the action will run an agent with cluade/openai/gemini (byok), second
   > option is to create a new bot that will run on each pr on all suit projects —
   > this will help sharing fixes and enhancements across them, adding consistency and
   > reducing future bugs"
   (typos "summerize", "cluade", "suit" are in the live comment.)

## 2. What the comment asks (fixes @echelon requested — inferred only from the thread)

From echelon's 09:29:01Z comment (only source; full text via the thread call above):

- **Praise**: "well built: automation ids for the caption buttons, translated tooltips,
  edge-resize zones, tests, and the `FILMCRAFT_DECORATIONS=1` escape hatch."
- **Rejection of scope**: undecorated-everywhere "affects X11, KDE, XFCE and other
  sessions too. Those users lose server-side decorations, window-manager theming,
  snapping/tiling behaviour and their configured button layout, and on Wayland the
  Minimize button disappears entirely. That's too broad a change for a default."
- **Two acceptable paths**:
  1. "winit's `wayland-csd-adwaita` feature (sctk-adwaita, pure Rust), as suggested in
     #433. It fixes the GNOME Wayland look ... and the change is much smaller."
  2. "If we want the single Premiere-style bar, make it **Wayland-only or opt-in** (a
     setting), with native decorations remaining the default elsewhere."
- **Cleanup**: "Please also drop the unrelated `.gitignore` `scratch/` line."
- **Community**: "please join our Discord: https://discord.gg/artcraft".

**Maintainer's plan** (origin comment): land those fixes, then explore "a more better
solution": an experimental/beta/optional **settings item users can manually enable**,
with **consistency across the whole storytold suite** as the stated goal.

## 3. @echelon identity

- `gh api users/echelon --jq '{login, type, bio, created_at}'` -> login `echelon`,
  **type "User" — NOT a Bot / not a GitHub App**, created_at 2008-12-09T08:19:15Z,
  id 39263, bio: "Founder: ArtCraft, Storyteller.ai, FakeYou. Previously: high-SLA,
  five nines, active-active, 50kqps services. Engineer, filmmaker. Rust."
- Profile page (fetched https://github.com/echelon, 2026-10-10): display name
  **"Brandon Thomas"**; Atlanta, Georgia, US; 1.1k followers; organizations include
  `storytold` (also spsu, dotfile, writings, lightengine, vocodes); pinned repo
  `storytold/artcraft` (Rust, 13.1k stars, 2k forks).
- Org membership verified: `gh api orgs/storytold/members/echelon` -> HTTP 204, and he
  is listed in `orgs/storytold/public_members`. His review comment has
  `author_association: "MEMBER"` and `performed_via_github_app: null`.
- The org: `gh api orgs/storytold` -> name "ArtCraft", description "Open Source Tools
  and Models for Makers, Builders, and Creatives", created 2021-01-03T07:27:05Z —
  matching echelon's bio ("Founder: ArtCraft").

- Activity (his profile contribution overview, window 2026-10-01..2026-10-11):
  commits in **19 storytold repos in ~10 days** — lightcraft 696, effectcraft 586,
  filmcraft 566, pdfcraft 446, photocraft 403, designcraft 350, soundcraft 162,
  vectorcraft 140, wordcraft 87, deckcraft 58, cadcraft 45, gridcraft 38,
  craft-launcher 18, craft-fonts 8, craft-repo-status-app 7, artcraft-services 6,
  photocraft-corpus 4, artcraft 3, craft-libs 2 (~3.4k commits). He also authors PRs
  (storytold/photocraft: "3 open 163 merged 4 closed"), e.g. #2378 "Format prefs_ui.rs
  (fixes main CI Format step)" and #2404 "Regenerate docs/scorecard.md after #2193".
- **Conclusion**: @echelon is the org's human founder/maintainer — the primary reviewer
  the new GitHub Action would assist or emulate. He is *not* an existing AI-reviewer
  bot; no GitHub App posts under his name. His inhuman commit volume plus
  filmcraft's own docs ("FilmCraft is built to be driven by AI agents, and largely
  built by them", docs/agents.md:3-4) indicate heavy AI-agent assistance under a human
  account — but that is inference; the account itself is a plain 2008 User.
- Web search attempt (`mcp__agy-cli-mcp__agy_web_search`) failed with HTTP 401 stale
  OAuth token; identity was established via the GitHub API and profile page instead.

## 4. Local filmcraft repo findings (/home/ahmed/Projects/filmcraft, read-only)

**Local checkout state**: branch `fix/target-dir-size` at 649ed3c (= maintainer PR #618
"build: line-tables-only dev debuginfo, incremental off, target hygiene"), clean tree.
PR #553's branch is *not* checked out; `FILMCRAFT_DECORATIONS` is absent from this
tree (consistent with the PR being unmerged) — `git branch`, `grep main.rs`.

**CI workflows** (`ls .github/workflows` — exactly 4 files; no dependabot.yml, no other
.github config):

- `freebsd.yml`: nightly cron `"37 2 * * *"` + PRs touching `packaging/freebsd/**` /
  `packaging/env.sh`; builds in a FreeBSD 14.3 VM (vmactions/freebsd-vm@ pinned sha),
  runs the release packaging script, checks the tarball; `contents: read`, no secrets.
- `packaging-lint.yml`: fast static gates — actionlint 1.7.12, shellcheck on
  packaging scripts, PowerShell parse, xmllint, desktop-file-validate, appstreamcli,
  Flatpak manifest cross-agreement, FreeBSD package dry-run; triggers on push/PR paths
  `.github/workflows/**`, `packaging/**`, `assets/app-icon/**`.
- `release.yml`: push to `release` branch only; jobs: version, macOS universal
  (signed/notarized), Windows x64/x86/arm64 (signed), Linux x86_64/aarch64 (AppImage,
  deb, rpm, tar.gz), Flatpak, FreeBSD, web/wasm -> draft GitHub Release; `release`
  environment holds signing secrets, all optional (missing -> unsigned + warning);
  every job checks out `storytold/craft-fonts` @ abb83316d9... pinned by sha and
  builds with `CRAFT_FONTS_DIR` + `CRAFT_FONTS_REQUIRED=1`.
- `windows-arm64.yml`: push to main + paths (Cargo.toml/lock, **/build.rs,
  packaging/windows/**) + weekly cron `"41 4 * * 1"`; cross-compiles the ARM64 MSI,
  then installs/runs/uninstalls it on a `windows-11-arm` runner; comment notes
  "PdfCraft saw an intermittent access violation in egui UI tests there" — existing
  cross-repo knowledge sharing, in comments.

**There is NO PR-triggered test/CI workflow in filmcraft.** The strict verification
gate is local, run before every commit: docs/contributing.md §3 "Quality gates" table —
`cargo fmt --check` (rustfmt.toml `max_width = 160`), `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace`, `cargo xtask layers`,
`cargo xtask assets`, `cargo xtask wasm`; `cargo xtask ci` runs them all (clippy and
tests with `--release` — xtask/src/main.rs:446-453). `cargo xtask` is an alias in
`.cargo/config.toml` for `cargo run -p xtask --`.

**Cross-suite inconsistency (live example)**: photocraft *has* GitHub-hosted CI that
filmcraft lacks — `/home/ahmed/Projects/photocraft/.github/workflows/` contains
`ci.yml` (Format: `cargo fmt --all -- --check`; Test: `cargo test --workspace --locked
--features heif`; Test without heif; Test with craft-fonts; Clippy `-D warnings`;
`cargo xtask layers`; `cargo xtask scorecard --check`; a corpus-tests job pinned by
sha256; an optional fuzz job), `docs.yml` (mdBook), `perf-nightly.yml` (cron
`"23 3 * * *"`, macOS Apple-GPU `cargo xtask perf` benchmarks). Filmcraft has none of
these three. echelon's own PR "Format prefs_ui.rs (fixes main CI Format step)"
(photocraft #2378) shows that CI runs on main there. This gap is exactly the kind of
thing the maintainer's "consistency in the whole storytold suite" asks for.

**Commit rules** (docs/contributing.md §4; CLAUDE.md): one task per commit, subject
starts with the task id (`M3.2: trim mode engine — ...`); commit only green states;
"AI agents end commit messages with the attribution line their environment requires."

**Agent conventions** (the rules any GitHub Action agent must obey in these repos):

- `AGENTS.md` applies "to every human and AI contributor"; `CLAUDE.md` holds working
  instructions and defers to AGENTS.md on conflict (AGENTS.md:3-4, CLAUDE.md:12).
- Never crash (AGENTS.md §0) **outranks feature work**: `Result<T,E>`, no
  `unwrap/expect/panic!/unreachable!/todo!/unimplemented!` outside tests, no `unsafe`
  (single exception `crates/platform`), hostile input numbers (`get()`, checked/
  saturating math), bounded recursion, `catch_unwind` on threads, panic hook as
  last resort, regression test for every crash fix; enforced by
  `#![deny(clippy::unwrap_used, ...)]` outside tests (clippy.toml allows in tests).
- Assets (AGENTS.md §1): never Adobe artwork; every asset needs `<file>.attribution`
  sidecar + `ATTRIBUTION.md` row, enforced by `cargo xtask assets`; ArtCraft brand
  marks are the one trademark exception (`docs/brand/`).
- Clean-room (AGENTS.md §2): no GPL/LGPL/AGPL code; codecs from public specs only;
  ffmpeg/ffprobe as external test oracles only, never linked/shipped.
- Layering enforced by `cargo xtask layers` (nothing below L5 depends on
  egui/eframe/winit/rfd/cpal); exact time = `Tick` (254 016 000 000/s); "everything is
  a command" (crates/engine: id, label, menu, shortcut, params, enabled(), run());
  "everything is agent-drivable" (automation ids on widgets, serde UI state).

- `docs/agents.md` — the canonical agent manual: MCP server in two modes
  (`filmcraft-cli mcp --demo` headless, `--bridge 127.0.0.1:9876` to the running app);
  repo `.mcp.json` registers both (`filmcraft`, `filmcraft-headless`); core tools
  `command_list`, `command_run`, `command_batch`, `doc_inspect`, `render_preview`;
  strict arguments (-32602 names the unknown argument); MCP progress/cancellation for
  long exports; control channel = JSON lines on 127.0.0.1 only; `ui.click`/`ui_drag`/
  `ui_key`/`ui_type`/`ui_screenshot` by automation id; "Verifying UI work" loop =
  drive -> assert on `ui.inspect`/`ui.elements` -> screenshot; autonomous work loop
  (§5: orient -> plan tests first -> implement -> verify all gates -> record); decision
  rules (§6: match professional-editor behaviour; ask a human only for licences,
  spending, publishing, personal data, destructive git, or AGENTS.md ambiguity);
  Definition of Done (§7); "Reporting progress honestly" (measured vs estimated;
  "don't raise a percentage without evidence a reviewer can rerun").
- Session protocol (CLAUDE.md): start from `plan/STATUS.md` (maintainer-local,
  gitignored) or public `ROADMAP.md`; parallel agents use separate worktrees and
  `CARGO_TARGET_DIR=target/agent-<name>`; `cargo xtask target-size --check` hygiene.
- Contributor credits (AGENTS.md bottom): an agent preparing a PR must check
  `contributors/people.toml` for its human's entry, ask once, add only their own; never
  hand-edit `contributors.json`; maintainers run
  `craftrules/scripts/contributors.py`.
- Cross-suite mechanism: standards live in `storytold/craftrules` (private; e.g.
  `standards/never-crash.md`, `standards/fonts.md`, `standards/test-corpora.md`,
  release playbook). Photocraft AGENTS.md:3 — "Standards and learnings shared across
  the crafting apps live in ../craftrules ... Contribute reusable learnings there,
  **never code; repos don't share code**." (`craft-libs` repo exists, 2 commits by
  echelon, per his profile — a possible future home for shared crates.)

**Bot activity in filmcraft**: none. `gh pr list --state all --limit 50` -> 23 unique
authors, and every one was verified with `gh api users/<login>` as **type "User"**
(including the AI-sounding logins BestCoder1877, 4papuce, MrCloudy2, seldoms). No
dependabot.yml, no GitHub App comments in the PR-553 thread. The proposed agentic
Action would be this org's *first* bot. Note some human accounts look agent-driven
(e.g. PR #619 "Premiere parity, text-based editing with local Whisper, and an MCP
server" by BestCoder1877) but that is unverifiable via the API — their type is User.

## 5. The experimental/beta/optional settings precedent in photocraft

The "behaviour" the maintainer says photocraft already has = the **System Title Bar**
preference — a persisted, user-toggleable, opt-in/out settings item controlling the
in-app title bar:

- `photocraft/crates/engine/src/prefs.rs:240` — `Interface.system_title_bar: bool`,
  default `false` (prefs.rs:266), doc comment: "Windows and Linux: use the system's
  title bar and window buttons instead of PhotoCraft's own one-row title bar (tiling
  window managers, desktops that draw their own decorations; #1271, #1316). Read when
  the app starts. macOS always uses the system's." Preferences are one plain serde
  document owned by `Session`; agents read/change it with `prefs.get`/`prefs.set`/
  `prefs.reset` (prefs.rs module docs, lines 1-14).

- `photocraft/apps/photocraft/src/main.rs:59` — `const CUSTOM_TITLEBAR: bool =
  !cfg!(target_os = "macos")` (custom bar is the default on Windows/Linux);
  `custom_titlebar(prefs_file)` (main.rs:64-68) reads the prefs JSON at startup:
  `CUSTOM_TITLEBAR && !prefs.interface.system_title_bar`; `native_options` sets
  `.with_decorations(!custom_titlebar)` (main.rs:83). Test
  `the_system_title_bar_preference_keeps_the_window_decorations` (main.rs:571-590)
  persists `{"interface":{"systemTitleBar":true}}` and asserts the decorations flag
  flips — the toggle is fully testable headlessly.
- `photocraft/crates/ui-egui/src/panels.rs:617` — `system_title_bar(app)` =
  `!app.custom_titlebar && prefs().interface.system_title_bar && !cfg!(any(target_os
  = "macos", target_arch = "wasm32"))`.
- `photocraft/crates/ui-egui/src/prefs_ui.rs` — the Preferences dialog is
  **schema-driven**: rows are generated from the serde prefs document, `Value::Bool`
  becomes a checkbox (lines 1264-1274); `interface.systemTitleBar` is special-cased to
  add an "Applies at next launch." hint (1268-1271); some prefs are hidden from the UI
  entirely (`performance` `useGpu`/`gpuBackend`/`renderingMode`, line 1256) — a
  hidden/advanced pattern adjacent to "experimental". Test:
  `system_title_bar_explains_next_launch_and_preserves_apply_and_cancel`
  (prefs_ui.rs:2540-2576).
- **How a new toggle is added (the recipe)**: add the field to the prefs struct with
  a doc comment + default; the dialog row comes free from the schema (plus any
  special-casing); wire consumption at startup (main.rs) or `apply_prefs`; add
  persistence + behaviour tests. Photocraft's scorecard audits "settings that do
  nothing" (AGENTS.md:16, docs/scorecard.md) — a dead toggle is a tracked failure.

**Filmcraft's counterpart today** (where the new experimental item would land):

- `filmcraft/crates/engine/src/settings.rs` — schema: `Category { id, title, rows }`,
  `Field { key, label, kind, indent, enabled_by, wired }` (lines 541-583); 16
  Premiere-order categories: general, appearance, audio, audioHardware, autoSave,
  color, graphics, labels, media, mediaAnalysis, mediaCache, memory, playback, plugins,
  timeline, trim (lines 677-1011). `wired: false` = "remembered for parity, no effect
  yet" (line 560) — a built-in dead-setting flag, like photocraft's scorecard audit.
- `filmcraft/crates/ui-egui/src/panels/settings.rs` — the Settings dialog edits a
  draft of the whole prefs document (serde, so `ui.inspect`/`ui.set` reach it), applies
  with the `prefs.set` engine command on OK; every control gets automation id
  `settings.<key>` (module docs, lines 1-15). `UiState.show_header` already exists and
  persists (crates/ui-egui/src/state.rs:389, default true at :873).
- **Gap**: filmcraft has no interface/window-chrome category and no title-bar setting;
  PR #553 uses the `FILMCRAFT_DECORATIONS` env var as the escape hatch instead. Adding
  the experimental item = a new prefs field + a `Row::Field` in settings.rs (likely a
  new `interface`/`experimental` category, `Kind::Bool`, `wired: true`) + consumption
  wiring + i18n entries + tests — the photocraft-consistent pattern.

## 6. Implications for the new agentic GitHub Action

The maintainer proposed the project himself (thread comment id 6097015929, quoted in
§1): **option (a)** a parent repo + GitHub cron Action running once or twice daily,
summarizing merged PRs / common patterns / duplicate code movable to reusable crates,
running an agent with claude/openai/gemini **BYOK**; **option (b)** a new bot running
on each PR across all suite projects. Stated goals: share fixes, add consistency,
reduce future bugs. Both architectures from the mission brief are confirmed as the
maintainer's own words; BYOK and the three providers are explicitly named.

Serving the exact PR-553 maintainer workflow would require the agent to:

1. **Verify PR claims by running the gates.** PR #553's body makes detailed, checkable
   claims (tests, clippy, rustfmt, live probes) and admits the full `cargo xtask ci`
   was not run. Filmcraft has no GitHub-hosted test CI (§4), so an Action agent that
   runs `cargo xtask ci` (needs a Linux runner with egui/winit system deps — see
   photocraft ci.yml's apt list — and the optional `CRAFT_FONTS_DIR` matrix) would add
   real verification value on every PR, not just summarize.
2. **Review like echelon**: assess cross-platform blast radius (X11/KDE/XFCE; WM
   theming, snapping, button layout, Wayland minimize), weigh the smaller `#433` option
   (`winit/wayland-csd-adwaita`) against the feature, and flag unrelated changes (the
   `.gitignore` scratch/ line). This needs code reading plus the issues/standards
   context — craftrules, sibling apps, linked issues (e.g. the same title-bar issue in
   gridcraft#54, referenced from issue #433's body).
3. **Propose cross-suite consistency changes**: the experimental/beta/optional settings
   item following photocraft's `systemTitleBar` precedent (§5); flag the
   filmcraft-vs-photocraft CI gap (no ci.yml/docs.yml/perf-nightly.yml in filmcraft).
   The comment's phrase "consistency in the whole storytold suite" is the recurring
   maintainer value — the agent should treat suite-wide pattern parity as a first-class
   review dimension.
4. **Comment structured, evidence-based reviews** (echelon's style: what is good, what
   is wrong, concrete acceptable alternatives, links to standards/issues) — and never
   merge: the human (MEMBER) stays the authority.
5. **If committing fixes**, obey AGENTS.md/CLAUDE.md fully: never-crash rules,
   attribution sidecars + `cargo xtask assets`, clean-room, layering, task-id commit
   subjects, green-states-only, commit attribution line, and the people.toml
   contributor-credits protocol for its human.
6. **Bot identity**: none exists today (§4) — the Action would be the org's first.
   A GitHub App would show as a bot account; author_association on its comments would
   be CONTRIBUTOR unless granted MEMBER. No in-suite precedent to copy.
7. **Screenshots**: the maintainer attaches screenshots to comments (two in the origin
   comment, not viewed here). The agent needs a policy: record URLs only, or
   single-file isolated vision review (the local user-level AGENTS.md also mandates
   isolated, single-image inspection for agents).
8. **Scheduling precedent exists**: filmcraft freebsd.yml nightly (`37 2 * * *`),
   photocraft perf-nightly (`23 3 * * *`), windows-arm64 weekly (`41 4 * * 1`) — a
   daily cross-repo consistency scan matches established in-org cron patterns. Org
   automation already exists in embryo: `craft-repo-status-app` (a status app repo,
   7 commits by echelon), `github-media`, `artcraft-services` (website).

## 7. Open questions

1. **Identity/permissions**: GitHub App bot vs PAT-backed account? Comment/review
   only, or push branches with fixes? (The origin comment says the maintainer will
   "land these fixes" himself — for PR review the bot may only need comment + workflow
   permissions. No in-org bot precedent exists to copy.)
2. **BYOK mechanics**: which providers exactly (comment names claude/openai/gemini),
   org vs repo secrets, who pays, and rate/usage caps per PR or per cron run.
3. **Option (a) shape**: a literal new parent repo, or an org-level scheduled workflow
   dispatching into the ~22 repos? Which repos are in scope (echelon's Oct activity
   covers 19 of them; the mission lists ~22)?
4. **Settings-item semantics**: one "Experimental" category per app, or a craftrules
   standard defining the toggle pattern (like standards/never-crash.md)? And note the
   default-direction tension: photocraft defaults to its custom bar (`system_title_bar:
   false`) while echelon asks filmcraft to keep native default + opt-in — does
   "consistency" mean aligning the mechanism only, or the defaults too?
5. **CI first?** Should the agent's first consistency fix be adding filmcraft's
   missing `ci.yml` (photocraft parity) so PR verification doesn't depend on the bot
   alone?
6. **Access**: the private `craftrules` repo holds the cross-suite standards the agent
   must cite — can the Action read it (same-org GITHUB_TOKEN? PAT?). `plan/` dirs are
   maintainer-local and gitignored, so the agent can never see those.
7. **Screenshot policy**: what do the two origin-comment screenshots show (not viewed
   per assignment)? Is vision review of user attachments in scope for the Action?
8. **Bug triage**: PR #553's body lists out-of-scope findings ("the `mp4.rs`
   allocation bomb, the `vaapi/hevc.rs` overflow, the snap-drift duplications, the
   dead i18n machinery, and the DMG sidecar") "to be reported or patched separately"
   — should the auto-file these as issues? That is a natural agent duty with a clear
   in-thread mandate.
9. **Existing bot-adjacent accounts**: some contributor accounts (e.g. BestCoder1877,
   who authored the MCP-server PR #619 and a conflict-resolved re-merge #631) may be
   human-driven agents; type is User, so the bot must not mistake them for automation
   or claim their work.

## Sources index

- gh api: comment 6096316169; issues/553/comments (thread, ids 6096129740/6096316169/
  6097015929); pulls/553/comments + pulls/553/reviews (empty); `gh pr view 553`
  (+ --json files); users/echelon, users/ahmedhosnypro, all 23 PR authors;
  orgs/storytold; orgs/storytold/members/echelon (204); public_members; issue #433
  (`gh issue view 433 --json ...`).
- Local: /home/ahmed/Projects/filmcraft/{AGENTS.md, CLAUDE.md, docs/agents.md,
  docs/contributing.md, .github/workflows/* (4 files), xtask/src/main.rs,
  crates/engine/src/settings.rs, crates/ui-egui/src/panels/settings.rs,
  crates/ui-egui/src/state.rs, .mcp.json, git state}; /home/ahmed/Projects/photocraft/
  {AGENTS.md, .github/workflows/ (7 files), crates/engine/src/prefs.rs,
  apps/photocraft/src/main.rs, crates/ui-egui/src/panels.rs,
  crates/ui-egui/src/prefs_ui.rs}.
- Web: https://github.com/echelon profile page (fetched 2026-10-10; contribution
  overview 2026-10-01..10-11). agy web search failed (401 stale token).
