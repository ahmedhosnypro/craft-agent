# craft-agent — task runner. Mirrored EXACTLY by .github/workflows/ci.yml
# (GH runners lack `just`); every change to a gate here must be mirrored
# there in the same change.

# cargo's incremental session dirs are never collected; gate builds skip them.
export CARGO_INCREMENTAL := "0"

# Show available recipes.
default:
    @just --list

# Full quality gate: fmt + clippy + tests + both scanners (with
# selftests) + file length + cargo deny + cargo machete + build --locked.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    ./scripts/scan-secrets.sh
    ./scripts/scan-secrets.sh --selftest
    ./scripts/identifier-scan.sh
    ./scripts/identifier-scan.sh --selftest
    ./scripts/check-file-length.sh
    cargo deny --locked check
    cargo machete
    cargo build --locked

# Live corpora: driven by the repo-root `.env` (TEST_BYOK_*); each recipe
# runs its `#[ignore]`d tests. When any knob is missing or empty the corpus
# SKIPs with a printed reason — never fails — so CI (which carries no
# `.env`) stays green. Not part of `just check`.

# Completions round-trip over the BYOK endpoint.
integration:
    cargo test -p craft-providers --test integration -- --ignored --nocapture

# Tool round-trip over the BYOK endpoint.
integration_tools:
    cargo test -p craft-providers --test integration_tools -- --ignored --nocapture

# Streamed tool-delta assembly.
integration_stream_tools:
    cargo test -p craft-providers --test integration_stream_tools -- --ignored --nocapture

# Bad-key auth path (typed error).
integration_auth:
    cargo test -p craft-providers --test integration_auth -- --ignored --nocapture

# Dialect mount over the same base URL.
integration_dialect_mount:
    cargo test -p craft-providers --test integration_dialect_mount -- --ignored --nocapture

# Full agent loop: real turn, tool dispatch, terminal record.
integration_agent_loop:
    cargo test -p craft-runtime --test integration_agent_loop -- --ignored --nocapture
