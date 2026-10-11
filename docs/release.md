# Release

Releases are tag-driven. Push a semver tag (`v1.2.3`) and the
[release workflow](../.github/workflows/release.yml) builds, verifies, and
publishes it; a `workflow_dispatch` run names an existing tag instead. Every
third-party action it uses is pinned to a commit SHA.

## Pipeline

1. **build** — one native runner per arch: `ubuntu-latest` builds
   `x86_64-unknown-linux-musl`, `ubuntu-24.04-arm` builds
   `aarch64-unknown-linux-musl`. Each leg runs
   `cargo build --release --locked -p craft-app -p craft-helpers
   --target <triple>` and uploads the binaries as an artifact.
2. **release** — runs only when every leg succeeded. It downloads the
   binaries, verifies the complete asset set exists, writes
   `checksums.txt` (`sha256sum` format, asset names in field 2), then
   publishes draft-first: create a draft release, upload all assets,
   publish. A failed run leaves at most a draft — never a partial
   published release.

## Assets

| Asset | Content |
| --- | --- |
| `craft-agent-x86_64-unknown-linux-musl` | static x86_64 Linux binary |
| `craft-agent-aarch64-unknown-linux-musl` | static aarch64 Linux binary |
| `craft-helpers-x86_64-unknown-linux-musl` | static x86_64 helpers binary |
| `craft-helpers-aarch64-unknown-linux-musl` | static aarch64 helpers binary |
| `checksums.txt` | `sha256sum` lines for all four binaries |

The asset names are the consumption contract: the Action maps
`Linux:X64`/`Linux:ARM64` runners to these triples, downloads the matching
binary plus `checksums.txt` from the same tag, and runs
`sha256sum --check` before installing.

## The moving `v1` tag

After publishing, the workflow repoints `v1` at the release commit and
force-pushes it. Pre-release tags (any tag containing `-`, such as
`v1.2.3-rc1`) never move `v1` — it always points at the latest stable
release. The workflow itself skips `v1` pushes so the repoint cannot
re-trigger a release.

## Consumer pinning

The Action's `version` input selects the release tag it downloads from:

- `version: v1` (default) — follows the latest stable release.
- `version: v1.2.3` — an exact tag, for reproducible runs; every asset and
  its checksums stay attached to that tag.

## Local parity

`scripts/build-release.sh` reproduces the build and `dist/` layout on a
development machine: same flags, `craft-agent-<triple>` plus
`checksums.txt` for the host (or a given triple, for cross-compile — see
the script header for the prerequisites).
