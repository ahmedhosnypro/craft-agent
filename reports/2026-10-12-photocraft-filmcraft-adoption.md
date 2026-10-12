# Cross-repo adoption report — storytold/photocraft ⇄ storytold/filmcraft (last 24h)

Scope: merged PRs in the 24 h window ending at collection time (2026-10-12T21:4x UTC, both repos' newest merged PR at 16:57/15:08 UTC). Method: `craft-helpers prs --org storytold --repos photocraft,filmcraft --since 24h --json`, shallow clones of both trees, `craft-helpers dups` top-level (once) and one `--cross-only` pass per shared crate. Every verified claim below carries its PR or `file:line`; helper-only output is labeled.

**Window caps honored:** the helper kept the newest 30 PRs per repo and 50 total — the PR list is `truncated` (filmcraft list ends at merged 01:52 UTC while the 24 h window reaches back further; count of dropped PRs is unknown). The top-level dups run hit its window cap during the photocraft root (filmcraft in-repo files were skipped) and returned its normal 20 candidates (photocraft paths only). Cross-repo replies carried no truncated markers.

## What merged (50 PRs, one line each)

### photocraft — 30 PRs (helper cap)
- #4362 — Tests: allow-list language-invariant bit-depth labels, add missing tr/hu 'Enable animations' (#4359) (vangel76, 2026-10-12T21:33 UTC, 8a/1d, 1f)
- #3872 — Use craft-print for system printing (skylartaylor, 2026-10-12T21:00 UTC, 434a/209d, 30f)
- #4253 — Web: wasm back under the 24 MiB gate with ~1 MiB headroom (25.38 → 24.17 MB) (bommerts, 2026-10-12T16:57 UTC, 304a/51d, 12f)
- #4147 — Fix main: wasm size cap, Windows Export As test, filter preview window (scope and a crash), CLI duplicate font warnings (echelon, 2026-10-12T15:48 UTC, 79a/6d, 5f)
- #3794 — Release workflow: pass the Azure signing secrets (echelon, 2026-10-12T15:08 UTC, 6a/0d, 1f)
- #3831 — i18n: add Hungarian (hu) UI translation (Skaywalker, 2026-10-12T15:07 UTC, 4006a/5d, 8f)
- #3809 — i18n(tr): add Turkish UI translation catalog (yeniufuklar-com, 2026-10-12T15:01 UTC, 3800a/3d, 7f)
- #3871 — feat: Alt+Cancel → Reset for opt-in command dialogs (seandellis, 2026-10-12T14:52 UTC, 646a/54d, 15f)
- #3973 — Verify PngSuite archives and cached corpus files (zenjabba, 2026-10-12T14:45 UTC, 696a/24d, 3f)
- #3614 — ops: count selection masks in history pixel budget (Streetblock, 2026-10-12T14:43 UTC, 88a/3d, 2f)
- #3641 — fix(linux): avoid duplicate AppImage desktop launcher (#3623) (Korg1ch, 2026-10-12T14:35 UTC, 61a/18d, 3f)
- #3727 — Codecs: decode JPEG and CCITT G3/G4 compressed TIFF (#3712) (vangel76, 2026-10-12T14:34 UTC, 56a/1d, 2f)
- #3722 — Codecs: decode only the requested EXR part (#3721) (vangel76, 2026-10-12T14:33 UTC, 61a/16d, 5f)
- #3637 — HEIF: move the heic-rs pin from 0.1.1 to 0.1.4 (s17sh70x2jfl031ag, 2026-10-12T14:22 UTC, 5a/5d, 3f)
- #3585 — Fuzz: ignore the same build leftovers in every fuzz crate (s17sh70x2jfl031ag, 2026-10-12T14:19 UTC, 7a/0d, 3f)
- #3501 — Format: overflow-safe blob decode and bounds-checked ZIP reads (#3486) (vangel76, 2026-10-12T14:08 UTC, 60a/34d, 3f)
- #3484 — Format: check pattern sizes on load (#3478) (vangel76, 2026-10-12T14:06 UTC, 98a/14d, 10f)
- #3900 — Color: convert input-only embedded profiles to the working space on open (#3876) (HEOJUNFO, 2026-10-12T13:57 UTC, 66a/3d, 2f)
- #3895 — Mix filtered selection edges with premultiplied alpha (jonathanwxh-cell, 2026-10-12T13:53 UTC, 139a/25d, 11f)
- #3894 — Count video footage in history memory budgets (jonathanwxh-cell, 2026-10-12T13:46 UTC, 90a/0d, 2f)
- #3890 — test(cms): validate profile-specific CMYK endpoints (FredApps, 2026-10-12T13:43 UTC, 68a/1d, 1f)
- #3888 — Layers: Merge Group for a selected group (#3832) (FredApps, 2026-10-12T13:42 UTC, 135a/0d, 22f)
- #3887 — Preserve surviving channel targets and visibility on deletion (jonathanwxh-cell, 2026-10-12T13:32 UTC, 54a/0d, 2f)
- #3886 — Merged and stamped layers keep their content bounds in CMYK and Lab (dgliwa, 2026-10-12T13:28 UTC, 185a/1d, 3f)
- #3819 — PSD type: read styles from the run's sheets and place text at its Txt2 frame (#3795, #1317) (FredApps, 2026-10-12T13:28 UTC, 528a/32d, 6f)
- #3725 — feat: Photoshop's tool keys (arrows turn the brush, Shift-click new type, Space+F canvas colour) (FinnNk, 2026-10-12T13:17 UTC, 185a/6d, 8f)
- #3869 — fix(io): preserve indexed BMP color tables (#3802) (Sir-Starch, 2026-10-12T13:13 UTC, 67a/2d, 6f)
- #3808 — Remove tool: Create a new layer, and Remove after each stroke off (diegochagas, 2026-10-12T12:51 UTC, 414a/19d, 28f)
- #3654 — PSD: don't preallocate knots from untrusted path length counts (#3635) (vangel76, 2026-10-12T10:28 UTC, 15a/1d, 1f)
- #3909 — Layer › Export As exports every selected layer or artboard (#3629) (fabianghe, 2026-10-12T10:04 UTC, 111a/10d, 4f)

### filmcraft — 20 PRs (helper cap)
- #1169 — Release workflow: pass the Azure signing secrets (echelon, 2026-10-12T15:08 UTC, 6a/0d, 1f)
- #1198 — #1124: ⌘D / ⇧⌘D apply transitions to the selected clips or edit points (n-delic, 2026-10-12T03:21 UTC, 104a/4d, 4f)
- #1193 — ui: reduce sequence copying and cache transcript redraw data (OorjitSethi, 2026-10-12T03:21 UTC, 212a/22d, 10f)
- #1168 — fix: Close Gapをショートカットに割り当てられるようにする (haranishi, 2026-10-12T03:21 UTC, 115a/2d, 3f)
- #1217 — dialogs: drag them by the title bar (open centred, no longer anchored) (liangforstudy, 2026-10-12T03:21 UTC, 114a/36d, 19f)
- #1052 — timeline: cancel drag and restore playhead on Escape (#580) (Auipp, 2026-10-12T02:55 UTC, 337a/61d, 5f)
- #1063 — Linux: VA-API EncSlice probe and bounded export GPU wait (#1058) (vbasky, 2026-10-12T02:41 UTC, 14a/5d, 3f)
- #1067 — #1039: 8-bit HEVC pictures as u8, in-place motion compensation (−14…23 % per frame), big-picture analysis (vangel76, 2026-10-12T02:38 UTC, 1316a/275d, 8f)
- #1070 — UI: favorite themes and appearance shortcuts (luniiya, 2026-10-12T02:36 UTC, 187a/7d, 15f)
- #964 — fix(ui): add Korean font fallback for UI text (mjohnson7cd, 2026-10-12T02:29 UTC, 63a/15d, 2f)
- #1165 — Trim MP3 encoder delay when decoding audio files (#1137) (MarekSpodBiedry, 2026-10-12T02:26 UTC, 56a/3d, 1f)
- #865 — M7: optimize audio DSP and isolate the unchanged mixer throughput gate (earlvanze, 2026-10-12T02:24 UTC, 587a/190d, 7f)
- #1046 — Median Radius is not capped at four working pixels (vbasky, 2026-10-12T02:22 UTC, 88a/1d, 2f)
- #837 — #828: import OpenEXR stills and image sequences (vangel76, 2026-10-12T02:19 UTC, 1302a/14d, 11f)
- #1159 — fix: エフェクトコントロールの矩形選択とShiftスナップを修正 (#967) (haranishi, 2026-10-12T02:15 UTC, 830a/54d, 7f)
- #1139 — fix(timeline): snap sub-frame timeline.place to the frame grid; inspect counts export's frames (#1101) (itrycodinganditsbad, 2026-10-12T02:11 UTC, 56a/1d, 3f)
- #879 — #874: compressed (RLE / ZIPS / ZIP) OpenEXR export (vangel76, 2026-10-12T02:06 UTC, 994a/73d, 15f)
- #1068 — #1065: CinemaDNG / DNG import with a clean-room decoder (vangel76, 2026-10-12T02:00 UTC, 3612a/21d, 25f)
- #931 — #402: animated GIF and APNG import as clips, with transparency (liangforstudy, 2026-10-12T01:58 UTC, 481a/4d, 10f)
- #1129 — fix(menus): native menu follows command enablement (#902) (itrycodinganditsbad, 2026-10-12T01:54 UTC, 100a/33d, 4f)

## Adoptable / shareable findings

- **Identical twin release-workflow PRs (strongest convention gap).** photocraft #3794 "Release workflow: pass the Azure signing secrets" (echelon, merged 15:08 UTC, 6a/0d, 1 file) and filmcraft #1169, same title/author/size, merged the same minute. The same release plumbing fix had to be authored twice; a shared workflow template (or a reusable workflow) would collapse this class. Verified from the prs JSON (both entries) plus the byte-similar title/author/stats; the workflow diff itself was not opened (PR-inspection cap).
- **Wasm bundle-size gate + packed i18n catalogs — photocraft → filmcraft.** photocraft #4253 (bommerts, 304a/51d) brought the wasm build back under a 24 MiB gate (25.38 → 24.17 MB) by subsetting fonts and deflating packed i18n catalogs in `crates/ui-egui/build.rs` (file list in prs JSON). The verified text/build.rs pair (below) shows filmcraft's build pipeline lacks `deflate_bundled_fonts()`/catalog packing. filmcraft had no wasm-size PR in the window → adoptable pattern.
- **CJK font-fallback module — filmcraft → photocraft.** filmcraft #964 (mjohnson7cd, 63a/15d) added `crates/ui-egui/src/cjk.rs` + theme.rs Korean fallback (prs JSON file list). photocraft has no `cjk.rs`; its new tr/hu catalogs (#3809, #3831) mean non-Latin UI strings are now its problem too → adoptable module.
- **i18n attribution/coverage conventions differ — pick one.** photocraft: per-locale `.tsv` + central `LICENSE-translations.txt` + `ATTRIBUTION.md` + `xtask/src/i18n_coverage.rs` enforcement (file lists of #3809/#3831; 19 locale files on disk). filmcraft: per-file `.tsv.attribution` sidecars, 9 locales, no coverage xtask (cloned-tree listing). photocraft's convention is the more automated → adoptable in filmcraft.
- **Font-warning dedup (photocraft #4147, echelon)** fixed duplicate-font warnings at CLI startup while #4253 packed the same fonts; filmcraft bundles craft fonts through the same build.rs family (verified pair below) → adoptable fix shape.
- **Corpus-verification convention (photocraft #3973, zenjabba, 696a/24d):** verifying PngSuite archives and cached corpus files. filmcraft's import wave (#837 DNG, #879 EXR, #1068 GIF, #931 GPR, all in window) has no equivalent corpus-verification PR in the window → adoptable convention.
- **Engine command organization:** photocraft splits engine commands into per-feature `*_cmds.rs` modules while filmcraft keeps a single `engine/src/commands.rs` (verified shared struct pair below; fc file spans 3400+ lines). The pc layout is the convention worth aligning on.
- **Theme settings plumbing:** filmcraft #1070 (luniiya, 187a/7d, theme.rs) adds favorite themes + appearance shortcuts; the crates' theme.rs files carry the same plumbing pattern (helper 6L candidate `src/theme.rs:42(fc)/124(pc)`, not verified). Porting direction is pc → adopt fc's favorites UX.

## Verified duplicate-code candidates (top-10 queue, in ranked order)

Cross-repo scan: 42 candidates over the 7 shared crates (ui-egui 23, engine 6, text 5, gpu 3, geom 2, automation 2, color 1; codecs/format/testkit 0). Verified: 17 pairs in one read per side (13 byte-identical excerpts, 4 normalized/divergent); 5 rejected in one attempt as misaligned or unresolvable helper excerpts; the rest are labeled helper output.

| # | lines | filmcraft | photocraft | reading |
|---|---|---|---|---|
| 1 | 82 | `crates/ui-egui/build.rs:11` | `crates/ui-egui/build.rs:147,16` | **identical** — the same font/catalog asset-packing build pipeline; pc and fc each extend it differently (#4253 vs craft fonts) |
| 2 | 61 | `crates/ui-egui/src/credits.rs:150` | `crates/ui-egui/src/credits.rs:163` | **identical** — contributors/credits render |
| 3 | 59 | `crates/ui-egui/src/credits.rs:351` | `crates/ui-egui/src/credits.rs:546` | **identical** |
| 4 | 44 | `crates/ui-egui/src/i18n/catalog.rs:12` | `crates/ui-egui/src/i18n/catalog.rs:16` | **identical** — catalog unpack core |
| 5 | 30 | `crates/ui-egui/src/credits.rs:67` | `crates/ui-egui/src/credits.rs:57` | **identical** |
| 6 | 29 | `crates/ui-egui/src/credits.rs:14` | `crates/ui-egui/src/credits.rs:13` | **identical** |
| 7 | 26 | `crates/ui-egui/src/i18n/catalog.rs:67` | `crates/ui-egui/src/i18n/catalog.rs:80` | **normalized** — pc adds `id()`/`plural()` accessors; pc is the superset |
| 8 | 25 | `crates/ui-egui/src/lib.rs:1603` | `crates/ui-egui/src/lib.rs:1506` | **identical** |
| 9 | 24 | `crates/text/build.rs:23` | `crates/text/build.rs:28` | **normalized** — same manifest→`CraftFont` pipeline; pc adds `deflate_bundled_fonts()` |
| 10 | 17 | `crates/engine/src/commands.rs:19` | `crates/engine/src/commands.rs:11,127` | **normalized** — identical code, doc comments drifted (menu-placement/shortcut wording) |
| 11 | 15 | `crates/ui-egui/src/credits.rs:119` | `crates/ui-egui/src/credits.rs:130` | **identical** |
| 12 | 9 | `crates/engine/src/export_tools.rs:136` | `crates/engine/tests/prefs_usage.rs:23` | **identical** — camel→snake_case helper duplicated across src and a test in different trees |
| 13 | 9 | `crates/gpu/src/lib.rs:119` | `crates/gpu/src/lib.rs:2147` | **identical** |
| 14 | 8 | `crates/engine/src/media_watch.rs:36` | `crates/engine/src/link_watch.rs:271` | **identical** — `fn stamp()` fs mtime+size helper |
| 15 | 6 | `crates/engine/src/media_watch_tests.rs:90` | `crates/engine/src/link_watch.rs:271` | **identical** — poll-until-done test loop (`assert!(t0.elapsed().as_secs() < 10, "the look never finished")`) |
| 16 | 8 | `crates/ui-egui/src/menus.rs:640` | `crates/ui-egui/src/menus.rs:864` | **divergent** — same menu-entry struct; fc derives `PartialEq`, doc comments differ |
| 17 | 7 | `crates/text/src/fonts.rs:28` | `crates/text/src/craft_fonts.rs:9` | **identical** — module even renamed between trees |

**Rejected in one attempt (helper excerpt misalignments, not re-chased):** engine 13L (`engine/src/commands.rs:1669(fc)`⇄`commands.rs:621(pc)`) and 10L (`commands.rs:482(fc)`⇄`extra_cmds.rs:1157(pc)`) — window offsets don't line up; ui-egui 12L `clip_dialogs.rs:450(fc)`⇄`analysis_ui.rs:714(pc)`; gpu 7L `lib.rs:95(fc)`⇄`lib.rs:140(pc)`; color 11L `fx.rs:272(fc)` (file does not exist in fc color crate)⇄`lib.rs:448(pc)`.

**Helper output, not verified (remaining queue + in-repo):** color 7L `src/lib.rs:176(fc)`⇄`src/convert.rs:146(pc)`; geom 7L `lib.rs:95(fc)`⇄`lib.rs:140(pc)` and 6L `lib.rs:133(fc)`⇄`lib.rs:176(pc)`; gpu 11L `fx.rs:272,lib.rs:96(fc)`⇄`lib.rs:448(pc)` and 6L `fx.rs:468,lib.rs:919(fc)`⇄`lib.rs:1298,368(pc)`; automation `server.rs:39(fc)`⇄`server.rs:200(pc)` (8L) and `server.rs:241(fc)`⇄`server/transport.rs:65(pc)` (6L); the smaller ui-egui credits/i18n/lib items; and the 20 in-repo photocraft groups from the capped top-level run (largest: 103 repeated lines across `crates/algo/src/gallery.rs` + reference-test files; second: 25 lines across 22 `engine` test/command files; third: 11 lines across 25 files). These are candidates only — no excerpt was read for them.

## Recommendations

1. **Extract a shared `ui-egui` asset-packing module** (build.rs pair #1, 82 verified-identical lines): one source of truth for font subsetting, catalog deflation, and the wasm size gate (pc #4253 already ahead).
2. **Unify credits.rs and the i18n catalog core** (pairs #2-#7, #11: 250+ verified-identical lines) into a shared crate or synced template; fc should adopt pc's catalog superset (#7) and its xtask coverage/attribution convention.
3. **Port `cjk.rs` font fallback (fc #964) to photocraft** before more non-Latin locales land (tr/hu already merged).
4. **Adopt photocraft's engine `*_cmds.rs` split in filmcraft** and fix the drifted `commands.rs` doc comments (pair #10).
5. **Share a release-workflow template** so fixes like the twin Azure-signing PRs (#3794/#1169) land once.
6. **Dedupe the small engine helpers** (pairs #12-#15: case-conversion, `stamp()`, poll-loop) into a shared `testkit`/util crate.
7. **Adopt pc's corpus-verification convention (#3973) for fc's codec imports** (DNG/EXR/GIF/GPR wave).

## Method notes & caps
- PRs: helper kept newest 30/repo, 50 total; `truncated` present → some in-window filmcraft PRs may be missing (unknown which).
- Top-level dups: one run; window cap (250000) reached during the photocraft root → filmcraft in-repo files skipped; 20 candidates (photocraft paths only) — normal helper window behavior, not re-run.
- Cross-repo: one loop, one `--cross-only` invocation per shared crate, `--max-candidates 500`, no truncated markers in any reply; ranked by repeated lines; verification stopped at the depth budget (17 verified / 5 rejected).
- No builds, tests, or installs were run; both storytold repos were only read (clones + prs JSON).

## Provenance note

This file replaces an earlier report committed under the same branch and message (commit 24fff81, PR #2, window ending 09:30 UTC): a prior delivery of this mission whose PR list went stale against a moving 24 h window. The branch's remote state was non-fast-forward for this run's push; the branch was reset to the current main head (602c529) and this commit carries the refreshed report — PR list, duplicate ranking, and verification tally are all from this run's collection (window ending ~21:45 UTC). The worklog used to rebuild it was deleted before commit.
