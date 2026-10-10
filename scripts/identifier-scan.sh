#!/usr/bin/env bash
# The no-lineage gate. Fails on:
#   1. whole-word harness identifiers, case-insensitive, anywhere
#   2. reference-action name strings, anywhere
#   3. provider host literals outside crates/craft-providers/
#
# The research record (analysis/, PLAN.md, scripts/progress-db.sql) names
# the studied systems by design and is the only legitimate place they
# appear; the scan skips it. This script's own pattern data and the
# selftest canary fixtures are the only other exempt text.
#
# Modes: (no argument) scan the repo; --selftest prove the scan on the
# canary fixture (every category must fire there) plus a clean pass over
# the repo. Exit codes: 0 = clean, 1 = findings OR a broken scan,
# 2 = usage error.
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

IDENTIFIERS='kimi|grok|xai|deepseek|dsh'
ACTION_REFS='claude-code-action|claude-code-base-action|codex-action'
HOST_LITERALS='api\.anthropic\.com|api\.openai\.com|generativelanguage\.googleapis\.com'
HOST_ZONE='^crates/craft-providers/'

# The files a commit could ship, minus the research record, this scan's
# own pattern data, and the selftest canary fixtures.
scan_file_list() {
  while IFS= read -r path; do
    [ -f "$path" ] && printf '%s\n' "$path"
  done < <(git ls-files --cached --others --exclude-standard |
    grep -v -e '^analysis/' -e '^PLAN\.md$' -e '^scripts/progress-db\.sql$' \
      -e '^scripts/identifier-scan\.sh$' -e '^scripts/fixtures/' || true)
}

# One grep pass. Word mode matches whole words only; substring mode
# matches anywhere in the line. Prints hits; rc 0 = the pass ran (hits or
# none), rc 2 = the scan itself is broken and the caller must fail closed.
grep_pass() {
  local label="$1" regex="$2" word="$3"
  shift 3
  local rc=0
  if [ "$#" -eq 0 ]; then
    return 0
  fi
  if [ "$word" = "word" ]; then
    grep -niwE -e "$regex" -- "$@" || rc=$?
  else
    grep -niE -e "$regex" -- "$@" || rc=$?
  fi
  if [ "$rc" -gt 1 ]; then
    printf 'identifier scan: grep failed rc=%s (%s)\n' "$rc" "$label" >&2
    return 2
  fi
  return 0
}

scan_tree() {
  local hits pass pass_rc
  local status=0 broken=0
  mapfile -t files < <(scan_file_list)
  if [ "${#files[@]}" -eq 0 ]; then
    printf 'identifier scan: no files found to scan\n' >&2
    exit 1
  fi
  # Provider host literals are adapter data: only craft-providers may
  # carry them, so every other file is checked.
  mapfile -t host_files < <(printf '%s\n' "${files[@]}" |
    grep -v -e "$HOST_ZONE" || true)
  for pass in identifiers action-refs host-literals; do
    hits=""
    pass_rc=0
    case "$pass" in
      identifiers)
        hits="$(grep_pass identifiers "$IDENTIFIERS" word "${files[@]}")" ||
          pass_rc=$? ;;
      action-refs)
        hits="$(grep_pass action-refs "$ACTION_REFS" substr "${files[@]}")" ||
          pass_rc=$? ;;
      host-literals)
        if [ "${#host_files[@]}" -gt 0 ]; then
          hits="$(grep_pass host-literals "$HOST_LITERALS" substr "${host_files[@]}")" ||
            pass_rc=$?
        fi ;;
    esac
    if [ "$pass_rc" -eq 2 ]; then
      broken=1
      continue
    fi
    if [ -n "$hits" ]; then
      printf 'identifier scan FAIL (%s):\n' "$pass" >&2
      printf '%s\n' "$hits" >&2
      status=1
    fi
  done
  if [ "$broken" -ne 0 ]; then
    printf 'identifier scan: broken scan — failing closed\n' >&2
    return 1
  fi
  if [ "$status" -ne 0 ]; then
    printf 'identifier scan FAIL: findings above\n' >&2
    return 1
  fi
  printf 'identifier scan OK: 0 harness identifiers, 0 action refs, 0 stray host literals (%s files)\n' "${#files[@]}"
}

if [ "$scan_mode" = "selftest" ]; then
  fixture="scripts/fixtures/identifier-canaries.txt"
  if [ ! -f "$fixture" ]; then
    printf 'identifier scan selftest: canary fixture missing: %s\n' "$fixture" >&2
    exit 1
  fi
  failed=0
  for word in kimi grok xai deepseek dsh; do
    if ! grep -niwE "$word" "$fixture" | grep -q "id-${word}:"; then
      printf 'identifier scan selftest: word %s did not fire on its canary\n' "$word" >&2
      failed=1
    fi
  done
  for aref in claude-code-action claude-code-base-action codex-action; do
    if ! grep -niE "$aref" "$fixture" | grep -q "action-${aref}:"; then
      printf 'identifier scan selftest: reference %s did not fire on its canary\n' "$aref" >&2
      failed=1
    fi
  done
  for host in api.anthropic.com api.openai.com generativelanguage.googleapis.com; do
    if ! grep -niE "$host" "$fixture" | grep -q 'host-'; then
      printf 'identifier scan selftest: host %s did not fire on its canary\n' "$host" >&2
      failed=1
    fi
  done
  # The allow zone must really be excluded from the host pass.
  mapfile -t zone_probe < <(scan_file_list)
  mapfile -t zone_outside < <(printf '%s\n' "${zone_probe[@]}" |
    grep -v -e "$HOST_ZONE" || true)
  for path in "${zone_outside[@]}"; do
    if printf '%s\n' "$path" | grep -q "$HOST_ZONE"; then
      printf 'identifier scan selftest: allow-zone file %s leaked into the host pass\n' "$path" >&2
      failed=1
    fi
  done
  if ! printf '%s\n' "${zone_probe[@]}" | grep -q "$HOST_ZONE"; then
    printf 'identifier scan selftest: no craft-providers files found to prove the allow zone\n' >&2
    failed=1
  fi
  if [ "$failed" -ne 0 ]; then
    printf 'identifier scan selftest FAILED — a category is unfalsifiable or the zone filter is broken\n' >&2
    exit 1
  fi
  if ! scan_tree; then
    printf 'identifier scan selftest FAILED — the tree scan is not clean\n' >&2
    exit 1
  fi
  printf 'identifier scan selftest OK: every canary category fires; zone filter holds; tree scan clean\n'
  exit 0
fi

if ! scan_tree; then
  exit 1
fi
