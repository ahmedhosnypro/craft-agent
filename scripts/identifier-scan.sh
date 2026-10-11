#!/usr/bin/env bash
# The no-lineage gate. Bans the studied harness identifiers and their
# host literals everywhere a commit could ship them:
#   - identifiers: exact tokens, case-insensitive, delimited by a line
#     edge or any non-alphanumeric char (hyphen/underscore/dot/slash all
#     count as delimiters)
#   - hosts: plain substrings, case-insensitive
# Bare provider tokens (claude, codex, kimi, grok, deepseek) and the
# provider hosts are legal by construction, so model ids, env names and
# adapter endpoints never trip.
#
# The research record (analysis/, PLAN.md, progress.db) is gitignored
# local-only material and never reaches the scanned set; the structural
# skips below only guard non-source paths. This script's own pattern data
# and the selftest fixture are the only other exempt text.
#
# Modes: (no argument) scan the repo; --selftest prove the scan on the
# canary fixture — every banned pattern must fire on its violation line,
# nothing may fire on a legal line, and a broken pass fails closed.
# Exit codes: 0 = clean, 1 = findings OR a broken scan, 2 = usage error.
set -euo pipefail
cd "$(dirname "$0")/.."

usage() {
  printf 'usage: %s [--selftest]\n' "$0" >&2
  exit 2
}

case "${1:-}" in
  "") scan_mode=tree ;;
  --selftest) scan_mode=selftest ;;
  *) usage ;;
esac
if [ "$#" -gt 1 ]; then
  usage
fi

# The ban list: harness identifiers as exact tokens, harness hosts as
# plain substrings.
BANNED_IDENTIFIERS=(
  kimi-code
  grok-build
  deepseek-harness
  xai
  dsh
  claude-code
  codex-action
)
BANNED_HOSTS=(
  api.x.ai
  api.moonshot.ai
  api.moonshot.cn
)

# Regex metacharacters escaped, so a ban-list token always matches itself.
literal_regex() {
  printf '%s' "$1" | sed 's/[.\\*+?(){}\[^$|]/\\&/g'
}

identifier_regex() {
  printf '(^|[^A-Za-z0-9])%s([^A-Za-z0-9]|$)' "$(literal_regex "$1")"
}

host_regex() {
  literal_regex "$1"
}

# The files a commit could ship: tracked paths (gitignore cannot hide a
# tracked file) plus untracked-but-not-ignored ones, minus the structural
# non-source paths, this scan's own pattern data, and the canary fixture.
scan_file_list() {
  local path
  while IFS= read -r path; do
    [ -f "$path" ] || continue
    case "$path" in
      .git/* | target/* | scratch/* | progress.db*) continue ;;
      scripts/identifier-scan.sh) continue ;;
      scripts/fixtures/identifier-canaries.txt) continue ;;
    esac
    printf '%s\n' "$path"
  done < <(git ls-files --cached --others --exclude-standard)
}

# One grep pass. PASS_HITS carries file:line pairs; rc 0 = the pass ran,
# rc 2 = the scan itself is broken and the caller must fail closed.
PASS_HITS=""
grep_pass() {
  local regex="$1"; shift
  local rc=0
  PASS_HITS=""
  if [ "$#" -eq 0 ]; then
    return 0
  fi
  PASS_HITS="$(grep -HniE -e "$regex" -- "$@" | cut -d: -f1-2)" || rc=$?
  if [ "$rc" -ge 2 ]; then
    return 2
  fi
  return 0
}

# One pattern over a file set. rc 0 = clean, 1 = findings (printed),
# 2 = broken pass.
pattern_pass() {
  local label="$1" token="$2" regex="$3"; shift 3
  if ! grep_pass "$regex" "$@"; then
    return 2
  fi
  if [ -z "$PASS_HITS" ]; then
    return 0
  fi
  printf 'identifier scan FAIL (%s %s):\n' "$label" "$token" >&2
  printf '%s\n' "$PASS_HITS" >&2
  return 1
}

scan_tree() {
  local token rc found=0 broken=0
  mapfile -t files < <(scan_file_list)
  if [ "${#files[@]}" -eq 0 ]; then
    printf 'identifier scan: no files found to scan\n' >&2
    return 1
  fi
  for token in "${BANNED_IDENTIFIERS[@]}"; do
    rc=0
    pattern_pass "banned identifier" "$token" "$(identifier_regex "$token")" "${files[@]}" || rc=$?
    if [ "$rc" -eq 1 ]; then found=1; fi
    if [ "$rc" -eq 2 ]; then broken=1; fi
  done
  for token in "${BANNED_HOSTS[@]}"; do
    rc=0
    pattern_pass "banned host" "$token" "$(host_regex "$token")" "${files[@]}" || rc=$?
    if [ "$rc" -eq 1 ]; then found=1; fi
    if [ "$rc" -eq 2 ]; then broken=1; fi
  done
  if [ "$broken" -ne 0 ]; then
    printf 'identifier scan: broken scan — failing closed\n' >&2
    return 1
  fi
  if [ "$found" -ne 0 ]; then
    printf 'identifier scan FAIL: findings above\n' >&2
    return 1
  fi
  printf 'identifier scan OK: 0 banned identifiers, 0 banned hosts (%s files)\n' "${#files[@]}"
}

FIXTURE="scripts/fixtures/identifier-canaries.txt"

# One pattern over the fixture, printing line:content hits; fails closed
# when the fixture grep cannot run.
fixture_grep() {
  local rc=0
  grep -niE -e "$1" "$FIXTURE" || rc=$?
  if [ "$rc" -gt 1 ]; then
    printf 'identifier scan selftest: cannot read the canary fixture\n' >&2
    exit 1
  fi
}

fixture_hits() {
  local token
  for token in "${BANNED_IDENTIFIERS[@]}"; do
    fixture_grep "$(identifier_regex "$token")"
  done
  for token in "${BANNED_HOSTS[@]}"; do
    fixture_grep "$(host_regex "$token")"
  done
}

# The legal corpus the fixture must always carry, else the legal direction
# of the selftest goes vacuous.
LEGAL_LABELS=(
  model-claude
  model-codex
  model-kimi
  model-deepseek
  bare-claude
  bare-codex
  bare-kimi
  bare-grok
  bare-deepseek
  env-openai
  env-anthropic
  env-google
  env-byok
  host-anthropic
  host-openai
  host-gemini
)

run_selftest() {
  local failed=0 token label stray
  if [ ! -f "$FIXTURE" ]; then
    printf 'identifier scan selftest: canary fixture missing: %s\n' "$FIXTURE" >&2
    return 1
  fi
  # Violation direction: every banned pattern fires on its labeled line.
  for token in "${BANNED_IDENTIFIERS[@]}"; do
    if ! fixture_grep "$(identifier_regex "$token")" |
        grep -E "^[0-9]+:ban-$(literal_regex "$token"):" >/dev/null; then
      printf 'identifier scan selftest: identifier %s did not fire on its canary\n' "$token" >&2
      failed=1
    fi
  done
  for token in "${BANNED_HOSTS[@]}"; do
    if ! fixture_grep "$(host_regex "$token")" |
        grep -E "^[0-9]+:ban-host-$(literal_regex "$token"):" >/dev/null; then
      printf 'identifier scan selftest: host %s did not fire on its canary\n' "$token" >&2
      failed=1
    fi
  done
  # Containment canary: the banned token inside a longer action name.
  if ! fixture_grep "$(identifier_regex claude-code)" |
      grep -E '^[0-9]+:ban-claude-code-action:' >/dev/null; then
    printf 'identifier scan selftest: claude-code did not fire on the claude-code-action canary\n' >&2
    failed=1
  fi
  # Legal direction: nothing outside a ban-* line may ever trip.
  stray="$(fixture_hits | grep -vE '^[0-9]+:ban-')" || true
  if [ -n "$stray" ]; then
    printf 'identifier scan selftest: a legal or comment line tripped the scan:\n%s\n' "$stray" >&2
    failed=1
  fi
  for label in "${LEGAL_LABELS[@]}"; do
    if ! grep -qE "^legal-${label}:" "$FIXTURE"; then
      printf 'identifier scan selftest: legal canary %s is missing from the fixture\n' "$label" >&2
      failed=1
    fi
  done
  # Fail-closed canary: a file grep cannot read breaks the pass.
  if grep_pass "$(identifier_regex kimi-code)" "${FIXTURE}.missing-probe" 2>/dev/null; then
    printf 'identifier scan selftest: an unreadable file did not fail the pass closed\n' >&2
    failed=1
  fi
  if [ "$failed" -ne 0 ]; then
    printf 'identifier scan selftest FAILED — a pattern is unfalsifiable or over-broad\n' >&2
    return 1
  fi
  if ! scan_tree; then
    printf 'identifier scan selftest FAILED — the tree scan is not clean\n' >&2
    return 1
  fi
  printf 'identifier scan selftest OK: every banned pattern fires; legal canaries pass; fail-closed holds\n'
}

if [ "$scan_mode" = "selftest" ]; then
  if ! run_selftest; then
    exit 1
  fi
  exit 0
fi

if ! scan_tree; then
  exit 1
fi
