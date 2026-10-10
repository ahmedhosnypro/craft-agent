#!/usr/bin/env bash
# Secrets scan (CWE-540). Exit codes: 0 = clean, 1 = findings OR a broken
# scan (a scan that cannot run must never pass silently), 2 = usage error.
#
# Output discipline: only file:line and the pattern name print — never the
# matched text, so the scan log cannot become the leak it is meant to catch.
#
# Modes:
#   (no argument)  scan every file a commit could ship: tracked files plus
#                  untracked-but-not-ignored ones. The gitignored live .env
#                  and scratch/ hold real local secrets and are never read.
#   --selftest     every family pattern must fire on its labeled canary
#                  line (and nowhere else in the fixture), and the normal
#                  scan must pass clean.
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

# High-signal credential patterns, dependency-free (plain grep).
secret_patterns=(
  "private_key:-----BEGIN [A-Z ]*PRIVATE KEY-----"
  "stripe_live_secret:sk_live_[A-Za-z0-9]{10,}"
  "stripe_restricted_live:rk_live_[A-Za-z0-9]{10,}"
  "aws_access_key_id:AKIA[0-9A-Z]{16}"
  "github_classic_token:ghp_[A-Za-z0-9]{30,}"
  "github_finegrained_token:github_pat_[A-Za-z0-9_]{20,}"
  "github_oauth_token:gh[os]_[A-Za-z0-9]{20,}"
  "slack_token:xox[abpr]-"
  "openai_classic_key:(^|[^A-Za-z0-9])sk-[A-Za-z0-9]{20,}"
  "openai_project_key:sk-proj-[A-Za-z0-9_-]{20,}"
  "anthropic_key:sk-ant-[A-Za-z0-9_-]{20,}"
  "google_api_key:AIza[A-Za-z0-9_-]{20,}"
  "stripe_webhook_secret:whsec_[A-Za-z0-9]{10,}"
  "dsn_embedded_credential:[a-z][a-z0-9+.-]*://[^/\"' ]+:[^/\"' @]+@"
)

# The files a commit could ship, minus this scan's own canary fixture
# (deliberate test data, never a finding). Tracked files stay listed even
# when gitignored, so a committed .env is still caught below.
scan_file_list() {
  while IFS= read -r path; do
    [ -f "$path" ] && printf '%s\n' "$path"
  done < <(git ls-files --cached --others --exclude-standard |
    grep -v -e '^scripts/fixtures/secret-canaries\.txt$' || true)
}

# .env-family rule: a tracked .env or .env.<suffix> is itself a finding;
# only the committed example/sample/template forms are allowed.
env_family_findings() {
  local path base
  while IFS= read -r path; do
    base="${path##*/}"
    case "$base" in
      .env.example | .env.sample | .env.template) continue ;;
      .env | .env.*)
        printf 'env-family file is tracked: %s\n' "$path"
        return 1
        ;;
    esac
  done < <(git ls-files --cached)
  return 0
}

work="$(mktemp)"
trap 'rm -f "$work"' EXIT INT TERM

scan_tree() {
  local found=0 entry name regex rc
  if ! env_family_findings; then
    found=1
  fi
  mapfile -t files < <(scan_file_list)
  if [ "${#files[@]}" -eq 0 ]; then
    printf 'secrets scan: no files found to scan\n' >&2
    exit 1
  fi
  for entry in "${secret_patterns[@]}"; do
    name="${entry%%:*}"
    regex="${entry#*:}"
    rc=0
    grep -nE -e "$regex" -- "${files[@]}" > "$work" || rc=$?
    if [ "$rc" -ne 0 ] && [ "$rc" -ne 1 ]; then
      printf 'secrets scan: grep failed rc=%s (pattern %s)\n' "$rc" "$name" >&2
      exit 1
    fi
    if [ -s "$work" ]; then
      printf 'secret pattern %s matched (file:line):\n' "$name"
      cut -d: -f1-2 "$work"
      found=1
    fi
  done
  if [ "$found" -ne 0 ]; then
    return 1
  fi
  printf 'secrets scan: clean (%s files)\n' "${#files[@]}"
}

if [ "$scan_mode" = "selftest" ]; then
  canaries="scripts/fixtures/secret-canaries.txt"
  if [ ! -f "$canaries" ]; then
    printf 'secrets scan selftest: canary fixture missing: %s\n' "$canaries" >&2
    exit 1
  fi
  declare -A labels=(
    [private_key]="private-key:"
    [stripe_live_secret]="stripe-live:"
    [stripe_restricted_live]="stripe-restricted:"
    [aws_access_key_id]="aws:"
    [github_classic_token]="github-classic:"
    [github_finegrained_token]="github-finegrained:"
    [github_oauth_token]="github-oauth:"
    [slack_token]="slack:"
    [openai_classic_key]="openai:"
    [openai_project_key]="openai-proj:"
    [anthropic_key]="anthropic:"
    [google_api_key]="google:"
    [stripe_webhook_secret]="stripe-webhook:"
    [dsn_embedded_credential]="dsn:"
  )
  failed=0
  for entry in "${secret_patterns[@]}"; do
    name="${entry%%:*}"
    regex="${entry#*:}"
    hits="$(grep -En -e "$regex" "$canaries" || true)"
    if ! printf '%s\n' "$hits" |
      grep -qE "^[0-9]+:[[:space:]]*${labels[$name]}[[:space:]]"; then
      printf 'secrets scan selftest: pattern %s did not fire on its canary\n' "$name" >&2
      failed=1
    fi
    stray="$(printf '%s\n' "$hits" |
      grep -vE "^[0-9]+:[[:space:]]*${labels[$name]}[[:space:]]" || true)"
    if [ -n "$stray" ]; then
      printf 'secrets scan selftest: pattern %s fired outside its canary\n' "$name" >&2
      failed=1
    fi
  done
  if [ "$failed" -ne 0 ]; then
    printf 'secrets scan selftest FAILED — a pattern is unfalsifiable or over-broad\n' >&2
    exit 1
  fi
  if ! scan_tree; then
    printf 'secrets scan selftest FAILED — the tree scan is not clean\n' >&2
    exit 1
  fi
  printf 'secrets scan selftest OK: every family canary fires; tree scan clean\n'
  exit 0
fi

if ! scan_tree; then
  exit 1
fi
