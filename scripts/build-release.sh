#!/usr/bin/env bash
# Local parity with .github/workflows/release.yml: the same cargo flags and
# the same dist/ layout (craft-agent-<triple>, craft-helpers-<triple> +
# checksums.txt).
#
# Cross-compile prerequisites: the target std (installed via rustup below)
# and a C toolchain for the target whenever the graph links C code — the
# vendored OpenSSL build needs a C compiler, perl, and make. Build through the
# rustup proxies in ~/.cargo/bin; a distribution rustc ignores
# rust-toolchain.toml.
# The release ships the craft-agent and craft-helpers binaries, so the
# build is scoped to their packages; a workspace-wide build would drag in
# members with glibc-linked dependencies.
set -euo pipefail

arch="$(uname -m)"
if [ "$arch" = "arm64" ]; then
  arch="aarch64"
fi
target="${1:-${arch}-unknown-linux-musl}"

rustup target add "$target"
cargo build --release --locked -p craft-app -p craft-helpers --target "$target"

rm -rf dist
mkdir -p dist
install -m 0755 "target/${target}/release/craft-agent" "dist/craft-agent-${target}"
install -m 0755 "target/${target}/release/craft-helpers" "dist/craft-helpers-${target}"
(
  cd dist
  sha256sum "craft-agent-${target}" "craft-helpers-${target}" > checksums.txt
  sha256sum --check checksums.txt
)

ls -l dist
