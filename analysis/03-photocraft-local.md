# ex3 — photocraft (local clone of storytold/photocraft): the org reference app

Clone: /home/ahmed/Projects/photocraft, origin https://github.com/storytold/photocraft.git (git remote -v), branch feat/pcraft-open @ 79bce578 (git log --oneline -5). Repo stats (gh api repos/storytold/photocraft): 39,320 stars, 5,724 forks, 748 open issues, default branch main, language Rust, created 2026-09-30. Analysis date 2026-10-10. Every claim below cites the file or command output it came from.

## 1. Repo shape

Top level (ls of repo root): apps/, assets/, book/, corpus/ (gitignored corpora), crates/, docs/ (30 .md files), examples/ (plug-in examples), packaging/, perf/ (budgets.toml, baseline.json), scorecard/ (8 per-area .toml checklists), scripts/, xtask/, contributors/, log/ (gitignored dev log), scratch/, plus AGENTS.md, README.md, ATTRIBUTION.md, clippy.toml, rustfmt.toml, Dockerfile, LICENSE-MIT, LICENSE-APACHE, NOTICE, SECURITY.md.

Workspace (Cargo.toml): members = ["crates/*", "apps/*", "xtask"]; default-members omits apps/photocraft-web. version 0.6.0, edition 2024, rust-version 1.95, MIT OR Apache-2.0; workspace lints unsafe_code = "forbid" (Cargo.toml:57-62).

Crate map (AGENTS.md §2; docs/architecture.md §2, :74-140):
- L0 foundation: geom, cms (ICC colour management), color, raster (pixel formats, blend math)
- L0 standalone, no workspace deps: psd, codecs, raw, affinity; tablet (the only unsafe crate: macOS AppKit event monitor in src/macos.rs)
- L1 doc (document model: layers, masks, adjustments, effects, smart objects)
- L2 ops (history), paint (brush engine), algo (imaging), text, vector
- L3 compose (CPU compositor = the oracle), gpu (wgpu compositor), format (.pcraft native bundle)
- L4 io (document <-> PSD/flat formats), plugins (sandboxed wasm plug-ins via wasmi)
- L5 engine: Session + command registry — every action is a command
- L6 ui-egui (thin egui shell; all actions go through the engine), automation (MCP server)
- plus testkit and heif (optional HEIC decoder wrapping heic-rs =0.1.1; crates/heif/Cargo.toml). crates/ holds 25 dirs (ls crates/).

Main app crate: apps/photocraft — package name "photocraft", default-run = "photocraft", "an open-source, native image editor" (apps/photocraft/Cargo.toml). Native eframe/wgpu desktop app with a TCP control server (apps/photocraft/src/control_server.rs; src also has crash_guard.rs, tablet.rs, mac_menu.rs, services.rs).

apps/photocraft-cli: headless CLI — convert, info, run, batch, commands, mcp, serve (AGENTS.md §2; docs/development.md:129-158). apps/photocraft-web: the same app compiled to wasm (trunk + wasm-bindgen), size-tuned [profile.wasm-release]: fat LTO, opt-level "s" (opt 3 for pixel crates), 24 MiB packaging cap (Cargo.toml:74-99).

Layering is machine-enforced by `cargo xtask layers`; a new crate must register in xtask/src/layers.rs (AGENTS.md §2). docs/architecture.md:240 states the litmus test for the seam: "apps/photocraft-cli and the automation server must be able to do *everything* the GUI does, except pointer painting", which they replay as PointerEvent streams.

## 2. CI workflows and gates

Seven workflows (ls .github/workflows/): ci.yml, docs.yml, freebsd.yml, packaging-lint.yml, perf-nightly.yml, release.yml, windows-arm64.yml.

**ci.yml** (push to main, all PRs, workflow_dispatch):
- `platforms` job picks the matrix per diff: plain PRs = Linux only; Windows is added when the diff touches apps/, crates/(ui-egui|text|gpu|tablet|heif)/, Cargo.lock, build.rs or ci.yml; the `full-ci` label forces Linux+Windows; macOS never runs on PRs (5 org macOS slots are kept for release.yml); the web job runs only for web-relevant diffs (ci.yml:33-73).
- `test` matrix: `cargo fmt --all -- --check` (ci.yml:110-112); `cargo test --workspace --locked --features heif` (:114-117); a without-heif run so HEIC stays a clear "not in this build" error (:118-122); a full workspace run with CRAFT_FONTS_DIR + CRAFT_FONTS_REQUIRED=1 against a pinned checkout of storytold/craft-fonts ref abb83316 (:129-142); `cargo clippy --workspace --all-targets --locked --features heif -- -D warnings` (:144-145); `cargo xtask layers` (:147-148); `cargo xtask scorecard --check` — docs/scorecard.md must match its sources (:152-154).
- `corpus`: `cargo xtask corpus --all` (pins in xtask/src/corpus_pins.rs, sha256-verified, cached by pin) then `cargo xtask test-corpus` (ci.yml:156-186).
- `wasm`: `cargo xtask wasm` — cargo check wasm32-unknown-unknown for the wasm-safe crates (:188-202).
- `web`: packaging/web/package.sh + a 24 MiB wasm size gate; sizes reported to the step summary (:204-246).
- `fuzz`: workflow_dispatch only; cargo-fuzz matrix over psd, codecs, affinity, format targets (:248-285).
- Concurrency: newer PR pushes cancel older runs; main runs are never cancelled (:25-30). Only main saves the cargo cache; PRs restore it (:102-108).

**docs.yml**: mdBook build on book/, docs/, README, SECURITY changes (push main + PRs). **freebsd.yml**: nightly (cron 17 2 * * *) plus PRs touching FreeBSD packaging; builds, tests and packages inside a FreeBSD 14.3 VM (vmactions/freebsd-vm). **packaging-lint.yml**: actionlint, shellcheck, AppRun integration, PowerShell parsing, desktop-entry/AppStream/Flatpak validation. **perf-nightly.yml**: nightly (cron 23 3 * * *) on a pinned macos-15 runner; `cargo xtask perf`; fails on an enforced perf/budgets.toml budget or a >15% p50 regression vs perf/baseline.json of the same machine class; never runs on PRs (perf-nightly.yml:1-23). **release.yml**: push to the release branch only; macOS universal (macos-15-xlarge), Windows x64/arm64, Linux; cargo xtask version set, signing, notarization, craft-fonts embedded (release.yml:9-11, 39-125). **windows-arm64.yml**: nightly (cron 41 4 * * *) + PRs touching Windows packaging; cross-compiles the ARM64 MSI, installs it on a Windows 11 ARM64 runner and runs the CLI (windows-arm64.yml:1-18).

Local release gate: `cargo xtask ci` = fmt --check -> clippy -D warnings -> test -> layers -> wasm, stops at the first failure (xtask/src/main.rs:27, 189-235). Other xtask commands: corpus, test-corpus, stats, parity, i18n-coverage, perf, scorecard, version, ico (xtask/src/main.rs:20-45).

So a PR must pass (at least on Linux): fmt, workspace tests with and without heif, tests with craft-fonts, clippy -D warnings, layering, scorecard freshness, corpus tests, wasm check — plus the web build/size gate and Windows tests when the diff touches those areas.

Branch rules: no branch protection on main (gh api repos/storytold/photocraft/branches/main/protection -> 404). One active ruleset (id 24322531) locks only refs/heads/release (creation, update, deletion, non-fast-forward) (gh api repos/storytold/photocraft/rulesets/24322531).

## 3. Agent-facing conventions (AGENTS.md and the automation-id question)

**AGENTS.md** (151 lines) is the first-read contract for AI agents and contributors:
- Product: pure Rust only — no Tauri/Electron/webview, no JS/TS; native egui/eframe on wgpu; the web build is the same Rust compiled to wasm (AGENTS.md:3). Product name "PhotoCraft"; siblings PascalCase ({Function}Craft); machine names lowercase (crates photocraft-*, ids ai.storyteller.photocraft).
- Shared standards live in ../craftrules (a private repo; outside contributors ask a maintainer). "Contribute reusable learnings there, never code; repos don't share code" (AGENTS.md:3).
- **Never crash outranks feature work**: no unwrap/expect/panic!/todo!/unreachable! in non-test code; no unsafe (sole exception photocraft-tablet/src/macos.rs); input-derived numbers are hostile (get() not [i], checked/saturating arithmetic, capped allocations); bound recursion; handle lock poisoning; last-resort panic guard around command dispatch; every crash fix ships a synthetic regression test; enforced by clippy.toml (tests only) plus per-crate #![deny(clippy::unwrap_used, ...)] (AGENTS.md:45-56).
- **Everything is a command**: new user-visible behaviour = an engine command in crates/engine/src/<area>_cmds.rs registered in commands.rs with id, label, menu path, shortcut, params doc, `enabled` and `run`, plus tests. UI, CLI, control channel and MCP all dispatch by id; use the exact id from crates/ui-egui/src/menu_catalog.rs (AGENTS.md:58; docs/contributing.md "Adding a command").
- Command params docs are explicitly agent-facing: "a params doc string such as {\"radius\":px=4,\"mode\":\"a|b\"=\"a\"} (this is what agents read through commands / command_list)" (docs/contributing.md step 3).
- No format/colour assumptions: bit depth and colour model are runtime data, conversions go through photocraft-cms; test at several depths (AGENTS.md:59).
- **Clean-room**: behaviour and look only; never copy proprietary code, shaders, profiles or assets; third-party assets need permissive licences + an ATTRIBUTION.md row in the same change; fonts are never committed — they live in storytold/craft-fonts, consumed via the optional CRAFT_FONTS_DIR build input, and everything must still work without it (AGENTS.md:60).
- **Tests are the gate**: every change comes with tests; PSD corpus results and the parity floor in crates/ui-egui/src/parity.rs must not regress (AGENTS.md:61).
- **UI is thin and data-driven**: UI state lives in ui-egui/src/state.rs (serde) so the control channel can read and drive it; colours/radii from theme::Tokens, never hard-coded (AGENTS.md:62).
- **Verify UI changes visually** and attach before/after screenshots to PRs: offscreen `cargo run -p photocraft-ui-egui --example snapshot` or launch with `--control` and take ui.screenshot (AGENTS.md:63; docs/contributing.md "UI" bullet).
- Never break wasm (`cargo xtask wasm`); performance is a feature (release-mode benchmarks, per-tile parallelism, timings recorded in the dev log); never panic on input — the panic_hunt integration test fuzzes every command with adversarial params and must stay green (AGENTS.md:64-67; crates/engine/tests/panic_hunt.rs).

Picking work: docs/roadmap.md "Honest parity assessment" first, then docs/scorecard.md (measured numbers, including "settings that do nothing"), docs/parity.md (missing menu items), log/devlog.md "Still open" bullets (AGENTS.md §4). The parity FLOOR in parity.rs may only rise, never lower (AGENTS.md:84).

"Before you finish a task" (AGENTS.md:88-100) — the exact pre-PR checklist: cargo test -p <touched crates>; clippy -- -D warnings; cargo xtask layers; xtask wasm (if L0-L6 touched); xtask parity (if commands added — commit the regenerated docs/parity.md); panic_hunt -- --ignored (if commands changed); xtask scorecard (if a measured number moved — commit the regenerated docs/scorecard.md, CI checks it); xtask perf --quick (hot paths); xtask test-corpus (psd/io/codecs/compose/gpu/text/format touched). Then append a terse entry to log/devlog.md — "the dev log plus a green tree is how the next agent picks up" (AGENTS.md:106).

Parallel-agent protocol (AGENTS.md §6): each agent uses its own target dir (CARGO_TARGET_DIR=target/agent-<name>) to avoid the Cargo build lock; edits only the files it owns; shared files (engine/src/lib.rs, the commands.rs v.extend list, ui-egui/src/menus.rs, state.rs) get small surgical edits, re-read before editing; new commands go in new modules; manifests are rewritten atomically (temp file outside crates/, then mv); finished agents delete their ~10 GB target/agent-* dirs.

Contributor-credit etiquette (AGENTS.md "Contributor credits"): an agent working for a contributor may add only its human's own [people.<username>] entry in contributors/people.toml (asked once); never add, edit, guess or copy anyone else's entry; contributors.json is generated and never hand-edited.

**Automation ids.** `rg -i -n "automation.?id" /home/ahmed/Projects/photocraft` finds no match: there is no separate "automation id" registry in photocraft. The equivalent, referenced throughout the code, is:
- Canonical command ids: crates/ui-egui/src/menu_catalog.rs holds CATALOG rows of (menu path, label, default shortcut, command id) for the entire Photoshop menu tree; an id that matches an engine or UI command goes live automatically (menu_catalog.rs:1-8; AGENTS.md:58). The scorecard reports 627 of 627 menu items live (docs/scorecard.md Automation row).
- CommandSpec params docs are the agent-facing API surface (docs/contributing.md step 3), served by engine.commands / command_list.
- Stable widget ids "so tests and agents can find them": caption button ids Id::new(("titlebar-caption", c as u8)) (titlebar.rs:33-36); controls named "for accessibility (and so tests and agents can find them)" (canvas.rs:1831, gradient_ui.rs:583, gradient_ui.rs:865, widgets.rs:757).
- docs/context-menu-inventory.md and context-menu-parity.md enumerate canvas context menus as ordered command ids (control-protocol.md:64 points agents at them).

## 4. Control / automation protocol (how an external agent drives the app)

Primary doc: docs/control-protocol.md (503 lines). Also book/src/automation/ (overview.md, cli.md, control-protocol.md, mcp.md, security-model.md) and docs/development.md "Driving the app programmatically" (:112-127) and "MCP (agents)" (:159-215).

Transport: the desktop app listens on 127.0.0.1:<port> (loopback only), started with `photocraft --control 7878 --control-token-file <path> --automation-read-root <dir> --automation-write-root <dir>` (control-protocol.md:9-16). Auth: a 256-bit hex bearer token; the first frame on every TCP connection must be {"method":"auth"}; env equivalents PHOTOCRAFT_CONTROL_TOKEN / _FILE; token files are 0600 on Unix (crates/automation/src/security.rs:21-27, TOKEN_BYTES=32).

Methods (control-protocol.md:41-76): engine.execute {command, params} runs any command by id (never opens dialogs; ui.menu.invoke mimics menu clicks); engine.commands lists the registry with enablement; ui.inspect (full UI state); ui.set (strict field whitelist, unknown fields error before anything changes); ui.menu.invoke / ui.menu.list; ui.dialog.open/set/confirm/cancel (agents read and drive dialogs like a user); ui.window.open/close; ui.pointer (document-coordinate pointer/pen events with pressure/tilt/rotation and modifiers — the same tool state machine as the mouse); ui.click/ui.move (screen points); ui.key; ui.type; ui.resize; ui.screenshot (PNG as base64 or a path under the write root; raises the window first because macOS stops rendering occluded windows); ui.focus; app.open/app.save (capability-scoped); app.quit.

Filesystem is capability-scoped: app.open/app.save work only under the launch-time read/write roots; absolute paths outside the root, parent traversal, alternate separators, drive prefixes and link escapes are rejected before any file effect; omitted authority fails closed (crates/automation/src/workspace.rs:15-40; control-protocol.md "MCP bridge" security note). Engine commands that use ambient filesystem paths are refused over automation ("use capability-scoped document methods") — every file.* except file.new, file.close* and a few path-free ones; prefs.set refuses whole-preference updates and the file-backed sections (colorSettings, scriptEvents, historyLog, plugIns, scratchDisks) so automation cannot widen file access indirectly (control-protocol.md:141). Transport limits (security.rs:22-28; control-protocol.md "Transport limits"): 1 MiB request line, 8 MiB reply, 16 connections, 30 s socket timeout, 256 batch steps. Long commands run as background jobs: pass "wait": false to get {job, pending: true}, then poll jobs.list / cancel with jobs.cancel (control-protocol.md "Background jobs (#210)").

Three ways an agent can connect (docs/mcp.md:3-16; docs/development.md:159-215):
1. JSON lines straight to the TCP socket (netcat example, development.md:115-127).
2. MCP over stdio via `photocraft-cli mcp` — headless (in-process engine session, no window) or bridge mode (`--bridge 127.0.0.1:7878 --control-token-file ...`) forwarding to the live app; built on the official rmcp SDK (crates/automation/src/lib.rs:1-15). MCP tools: command_list / command_run / command_batch, doc_open/new/save/export/inspect/render_preview/select/close, session_list, jobs_list/cancel; bridge-only: ui_inspect, ui_screenshot, ui_pointer, ui_menu_invoke, ui_set, control_call (any method). photocraft://document and photocraft://commands resources return live JSON (docs/mcp.md).
3. Headless JSON-lines server `photocraft-cli serve` (stdio or loopback TCP) with a batch method (control-protocol.md "Headless server").

docs/mcp.md:3: "PhotoCraft follows the same conventions as FilmCraft #28" — the MCP/tool conventions are already shared across the suite. The CLI also exposes one-shot agent flows: run --new/--cmd/--out, batch --actions actions.json (docs/development.md:129-158).

Visual-verification paths an agent (or a CI Action) can use:
- Offscreen, no window, no focus stealing: `cargo run -p photocraft-ui-egui --example snapshot -- --out ui.png --size 1440x900 --scale 2 --open photo.jpg --script '[[method, params], …]'` — the script is an ordered list of control-protocol calls; flags include --custom-titlebar (draw the Windows/Linux title bar), --system-theme, --safe-gpu (CPU canvas), --monitor/--window-top (simulated display) (crates/ui-egui/examples/snapshot.rs:1-27).
- Live app: ui.screenshot over the control channel, or MCP ui_screenshot / doc_render_preview (bounded PNG content, max_side 2048).
- egui_kittest headless UI tests (crates/ui-egui/Cargo.toml:48; docs/architecture.md §13).
- Acceptance-test pattern: crates/automation/tests/agent_tasks.rs drives 10 realistic edit tasks purely over MCP and verifies via doc_inspect / document.pixel (roadmap M11 DoD; development.md:203-215 documents the typical agent loop: doc_open -> command_list -> command_run -> doc_render_preview -> doc_save).

## 5. The experimental / optional settings mechanism

There is no literal "experimental"/"beta" flag in the crates (rg -i "experimental|beta" crates/ matches only math betas, a paper title and test fixture names; nothing in docs/roadmap.md either). The behaviour the maintainer wants filmcraft to adopt is photocraft's preferences system and its gating:

**Where settings live**: crates/engine/src/prefs.rs (1885 lines). "The whole set is plain serde data owned by the Session, so every frontend reads the same values and agents change them with prefs.get / prefs.set / prefs.reset" (prefs.rs:1-9). Sections mirror Photoshop's Preferences dialog (SECTIONS list, prefs.rs:933-944); each section is a struct with #[serde(default, rename_all = "camelCase")] fields (General, Interface, Workspace, Tools, Performance at prefs.rs:169, 231, 268, 520). Choices are validated enums generated by a `choice!` macro with JSON names (prefs.rs:23-38); choices()/range() validate enums, ranges and #rrggbb colours (prefs.rs:1010-1060).

**The gating that makes an unwired setting invisible — the "experimental/optional settings item" mechanism**: HIDDEN_UNTIL_IMPLEMENTED, a const list of preference paths "that nothing reads yet, so Edit > Preferences doesn't show them: a control that does nothing is worse than a missing one (issue #204). They still load, save and round-trip through prefs.get/prefs.set unchanged. When you implement one, remove it here; the prefs_usage test fails while a listed preference is read anywhere outside prefs.rs, and while an unlisted one is read nowhere" (prefs.rs:945-953). is_hidden(path) = HIDDEN_UNTIL_IMPLEMENTED plus LINUX_ONLY platform gating (performance.linuxDisplayServer shows only on Linux) (prefs.rs:1004-1012). So an "optional" pref exists in the schema and round-trips for automation, but stays out of the UI until something reads it; wiring PRs then remove the entry (e.g. PR 2543 "Place: wire Always Create Smart Objects When Placing (#204)", PR 2531 "Interface: wire Dynamic Color Sliders (#204)", PR 2522 "Type: wire Use Smart Quotes (#204)" — gh pr list).

**Scorecard enforcement**: docs/scorecard.md is generated by `cargo xtask scorecard` from scorecard/*.toml, perf/budgets.toml, corpus floors and a prefs audit; it reports "settings that do nothing: 46 of 149 (target 0)" and CI fails if the file is stale (docs/scorecard.md Summary; ci.yml:152-154; xtask/src/scorecard.rs:549, 601).

**Staged application**: settings that need a restart carry "applies at next launch" doc comments (gpu_backend, use_gpu, cache_tile_size, linux_display_server — prefs.rs:530, 532, 536, 542) and a faint "Applies at next launch." label in the dialog (prefs_ui.rs:1062), with tests asserting the hint appears only for those (prefs_ui.rs:2554-2587). Legacy values are migrated, not dropped: e.g. Performance::effective_rendering_mode resolves old useGpu/gpuBackend into the new explicit rendering_mode without letting legacy flags override it (prefs.rs:553-556).

**Storage**: desktop writes preferences.json to the platform config dir (macOS ~/Library/Application Support/Photocraft, Windows %APPDATA%\Photocraft, Linux $XDG_CONFIG_HOME/photocraft; override PHOTOCRAFT_CONFIG_DIR; portable mode uses PhotoCraftData/) plus ui.ron for window geometry; the web build uses localStorage; saves are three-way merged so two instances never clobber each other, and failed writes retry with backoff (control-protocol.md "Preferences"). The UI layer only edits a working copy and commits through prefs.set on Apply/OK (prefs_ui.rs:1-9).

**Agent-facing prefs rules**: prefs.set takes dotted "section.key" paths, validates choices/ranges/colours, applies batches all-or-nothing; over the control channel it refuses whole-preference updates and file-backed sections; the Preferences dialog itself refuses Apply/OK over control and points agents at prefs.set (control-protocol.md:60, Preferences table).

**Optional-build settings**: the optional `heif` cargo feature is the build-level analogue — official builds compile with --features heif (apps/photocraft/Cargo.toml [features]), CI tests both with and without it, and the codecs produce a clear "not in this build" error for distributors who leave it out (ci.yml:114-122).

**Agent-related prefs that exist but are deliberately inert**: integrations.allowAgentControl and integrations.controlPort sit in HIDDEN_UNTIL_IMPLEMENTED with the comment "Agent access is governed by the launch flags (`--control`, the automation roots), not by these yet" (prefs.rs:999-1001). Agent access today is launch-flag-scoped, not a preference.

## 6. Title bar reference implementation (what filmcraft PR 553 copies)

crates/ui-egui/src/titlebar.rs (468 lines) is the custom title bar for Windows/Linux, where the window has no OS decorations: the app's top bar (panels::title_bar, panels.rs:492) is the title bar, "as in Photoshop on Windows" (titlebar.rs:1-6).
- Caption buttons: BUTTON_WIDTH 46 pt; three buttons (Minimize, Maximize/Restore, Close) flush with the window's right edge so a maximized window's corner hits Close; Close dispatches the file.exit command (asks about unsaved documents first), Minimize/Maximize send egui ViewportCommands (titlebar.rs:14-21, 50-89).
- The close button's red is a theme token: Tokens.caption_close = rgb(196,43,28) and caption_close_text = white (theme.rs:147, 218-219); hover/press adjust via gamma multiply; the other buttons use t.hover/t.pressed (titlebar.rs:59-67).
- Stable ids for agents/tests: caption_id = Id::new(("titlebar-caption", c as u8)), "stable, so tests and agents can find them" (titlebar.rs:33-36).
- Resize zones: invisible 8 pt edge (EDGE) + 12 pt corner (CORNER) zones drawn in a foreground layer; a press sends ViewportCommand::BeginResize; no zones while maximized/fullscreen; one-sided cursor names because some cursor themes lack ew-resize/ns-resize (issue #1581) (titlebar.rs:18-21, 115-134, 183-202).
- KDE Wayland compositor hand-off: the compositor keeps the resize's button release and cursor, so release_after_os_resize synthesizes the release and reshow_cursor_after_os_resize re-shows the cursor for one frame (titlebar.rs:136-179); both behaviours have regression tests (titlebar.rs:374-420).
- The bar is fully tested headlessly: caption placement at 760/1100/1440 widths with no widget overlap, brand mark leading the bar, drag-gap + double-click-maximize, close == File > Exit, edge-zone resize/cursor behaviour — via a Win harness that runs raw egui frames with no window (titlebar.rs:204-467). A port (filmcraft PR 553) should copy the tests with the code.
- The bar itself (panels.rs:492-621): brand mark left, menus, centred document title (shortened or dropped rather than overlapped), workspace controls; 32 pt tall in the pro theme, 38 pt otherwise; macOS uses the system menu bar + integrated_titlebar with a 78 pt inset (panels.rs:495-497).
- Escape hatch preference: interface.systemTitleBar (prefs.rs:240) lets Windows/Linux users take the OS title bar (tiling WMs etc.); system_title_bar() resolves it, macOS always uses the system's (panels.rs:621-622).
- Offscreen verification: `--custom-titlebar` makes the snapshot example draw the Windows/Linux title bar, so title-bar changes can be verified in CI without Windows (snapshot.rs:16).

## 7. Bot activity

`gh pr list --repo storytold/photocraft --state all --limit 50 --json number,title,author`: the 50 most recent PRs (#2477-#2565) are all by human accounts. Checked with `gh api users/<login> --jq .type` for the top authors (MarekSpodBiedry 9 PRs, davidmann4 8, Ni-zav 5, MrCloudy2 5, vangel76 4, reynold-hu 4, bitshiftza 4, ahmedhosnypro 4, KausBoss 3, harrisonoconnorhover 3, Erica4fr 3, Elvis33LE 3, echelon 3, Yassinsherif14112010 1): every one returns type "User"; no Bot accounts, no dependabot/renovate/stale bot appears in the PR list.

- The maintainer (ahmedhosnypro) authors the automation PRs himself: MCP bridge reconnect/idle-socket retry (#2507), control timeout errors naming likely causes (#2497), CLI PHOTOCRAFT_CONTROL_PORT fallback (#2495), app.save encode off the UI thread (#2517) (gh pr list output).
- One suspicious PR: #2523 "Arena/e2409cc4 photocraft" by Yassinsherif14112010 — generic title, empty body, still OPEN; the account was created 2024-11-01 (gh api users/Yassinsherif14112010). Looks like drive-by/spam or an automated dump; the org currently handles such PRs by hand, with no bot policy in evidence.
- The PR stream is fast-moving and human: dozens of small feature/fix PRs, each titled with the Photoshop behaviour and an issue number (e.g. #2564 Select > Modify > Border, #2557 Select > Modify > Smooth, #2543 Place: wire Always Create Smart Objects) — an issue-by-issue contributor community an agentic Action would review alongside and post to.
- The repo shows agents already work here daily: the latest devlog entries describe an agent session driving the app end-to-end over the control channel (generative artwork, three audit iterations) and root-causing a DPMS-off wedge of the control channel; AGENTS.md and the devlog are written as agent operating instructions (log/devlog.md entries 2026-10-10).

## 8. What an org-wide agentic GitHub Action must respect in photocraft (and siblings)

Hard gates a bot-authored PR must pass (ci.yml + AGENTS.md):
- cargo fmt --check; clippy --workspace --all-targets -- -D warnings; tests with and without --features heif; corpus tests; cargo xtask layers; cargo xtask wasm; cargo xtask scorecard --check (docs/scorecard.md is generated — regenerate, never hand-edit); docs/parity.md regenerated via cargo xtask parity whenever commands change.
- Windows enters the matrix automatically when the diff touches apps/, crates/(ui-egui|text|gpu|tablet|heif)/, build.rs or Cargo.lock, and the web build runs when crate sources change (ci.yml:37-73): a UI-only PR still costs a Windows job and a fat-LTO wasm build.
- Never-crash style is lint-enforced: no unwrap/expect/panic/todo/unreachable/unsafe in non-test code (clippy.toml + workspace lints); commands must return Err on any params — the panic_hunt fuzz test must stay green (AGENTS.md:49-56, 67).
- Every change ships tests, including bad-params and multi-depth cases; UI changes attach before/after screenshots to the PR (docs/contributing.md).
- Naming: user-facing "PhotoCraft" ({Function}Craft PascalCase) vs lowercase machine names; no Tauri/Electron/JS/TS/webview deps; no font files committed (craft-fonts repo instead); assets need licences + ATTRIBUTION.md rows.
- New settings: add the serde field + section; wire it in the same PR or add the dotted path to HIDDEN_UNTIL_IMPLEMENTED (the prefs_usage test fails otherwise); restart-gated ones document "applies at next launch" (§5 above).
- New commands: id from menu_catalog.rs (or a descriptive id in the same style), params-doc string, enabled predicate, exactly one history step, tests; registered via specs() in a new engine module (docs/contributing.md "Adding a command").
- Parallel-agent hygiene: own target dir, surgical shared-file edits, atomic manifest writes (AGENTS.md §6) — relevant if several Action instances run against one repo; each target dir is ~10 GB.
- Comments: terse, explain why, match surrounding style (docs/contributing.md "Style"; repo comments never carry dates/incident history).

What an org-wide Action can reuse from photocraft:
- The control protocol + MCP server as the canonical way to drive any suite app headlessly; docs/mcp.md:3 says FilmCraft #28 already follows the same conventions, so the Action can assume one client shape across apps. An Action can build photocraft-cli, run `photocraft-cli mcp` headless with automation roots pointing at the PR checkout, execute commands, and produce PNG evidence with no display at all (the snapshot example renders the full UI offscreen; snapshot.rs:1-3).
- ui.screenshot / doc_render_preview for evidence images; doc_inspect / document.pixel to verify edits without screenshots; the documented agent loop is doc_open -> command_list -> command_run -> doc_render_preview -> doc_save (docs/development.md:203-215).
- The snapshot example's `--script '[[method, params], …]'` = deterministic before/after image generation for PR comments, including --custom-titlebar for title-bar work and --safe-gpu for CPU-path rendering (snapshot.rs:1-27).
- The prefs registry pattern — serde structs + choices()/range() validation + HIDDEN_UNTIL_IMPLEMENTED gating + prefs_usage test + scorecard "settings that do nothing" metric — as the cross-suite template for the consistency work the maintainer wants (filmcraft adopting photocraft's experimental/optional settings behaviour; §5).
- titlebar.rs plus its headless test suite as the reference implementation other apps port (filmcraft PR 553); the theme Tokens (caption_close), the systemTitleBar preference and the KDE Wayland hand-off fix travel with it (§6).
- `cargo xtask ci` as the local mirror of CI: the Action can run the identical gate before pushing a branch (xtask/src/main.rs:189-235).
- AuthorizedWorkspace (cap-std capability directories, independent read/write roots, fail-closed) as the security model for anything the agent reads or writes on disk; ambient-path engine commands already refuse automation (crates/automation/src/workspace.rs).
- agent_tasks.rs as the template for agent acceptance tests: task-driven, MCP-only, verified through inspect commands rather than internals (crates/automation/tests/agent_tasks.rs:1-9).
- The dev-log convention (log/devlog.md) as agent session memory, and AGENTS.md as the per-repo contract the Action must parse and obey first.

## 9. Open questions

- ../craftrules is private; AGENTS.md:3 tells outside contributors to ask a maintainer for the standards relevant to their change (never-crash.md, fonts.md, release/playbook.md). An org-wide Action cannot read it — which standards must be inlined, mirrored or summarized for the agent to obey?
- How uniform are these conventions across the other ~21 repos? docs/mcp.md cites "FilmCraft #28" for MCP conventions, but is there a shared doc each repo pins, or does each restate it? (Needs the other repos to answer.)
- No "automation id" registry exists (§3). If the new agent wants stable element ids for UI manipulation across apps, it must rely on per-app command ids plus egui widget-id conventions; is a cross-repo id convention planned?
- GPU-canvas changes: the snapshot example has --safe-gpu (CPU path) and CI uses software adapters for GPU parity (docs/architecture.md §13), but what is the sanctioned CI story for pixel-perfect GPU-rendered screenshots in a headless Action?
- macOS is tested only in release.yml (ci.yml:53-55 comment: the org's 5 macOS slots are kept for release builds), so an agent touching macOS-specific code gets no PR-level signal.
- Perf budgets/baselines exist only for macOS Apple silicon and run nightly, never on PRs (perf-nightly.yml) — the Action must not claim perf verification it cannot run.
- BYOK model access: nothing in photocraft constrains agent API keys, but the control-token security rules (loopback only, token files 0600, never log or commit tokens, do not put tokens on shared process command lines — control-protocol.md security note) are the local pattern the Action should copy for key handling.
- Bot etiquette is undefined: PR #2523 (generic "Arena/e2409cc4 photocraft", empty body) sits OPEN, and the repo has no bot account, triage policy or automation conventions — the new Action would be the org's first, so its review/comment/commit conventions will set precedent and must match the maintainer's issue-numbered, behaviour-titled PR style.
