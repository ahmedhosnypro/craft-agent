#!/usr/bin/env bash
# File-length gate: no first-party Rust file may exceed 300 effective
# lines. Effective lines are counted with blank lines and comment lines
# skipped (line comments and block comments, the block state carried
# across lines). Test code is included; there is no per-file override
# list and no inline escape — an oversized file is a gate failure and
# the remedy is splitting the file. The function half of the limit
# (75 effective lines) rides on clippy (too-many-lines threshold in
# clippy.toml); clippy cannot express a per-file cap, so this script is
# the file half.
#
# Fast and dependency-free (bash + awk + find only).
#
# Optional checker mode: `check-file-length.sh <path>...` checks only the
# named files (the per-file loop a refactorer runs while splitting one
# file). With no args it scans every first-party root — the gate
# invocation.
set -euo pipefail
cd "$(dirname "$0")/.."

LIMIT=300
# Roots scanned — keep in lockstep with the AGENTS.md line-count law.
ROOTS=(crates)

# effective_lines <file>: lines that are neither blank nor part of a
# comment. A line counts only when its first non-space character starts
# real code; `//`, `///`, `//!` line comments and `/* ... */` block
# comments (multi-line state carried across lines) are skipped.
effective_lines() {
  awk '
    BEGIN { in_block = 0; n = 0 }
    {
      s = $0
      sub(/^[ \t\r]+/, "", s)
      sub(/[ \t\r]+$/, "", s)
      if (in_block) {
        if (s ~ /\*\//) in_block = 0
        next
      }
      if (s == "") next
      if (s ~ /^\/\//) next
      if (s ~ /^\/\*/) {
        if (s !~ /\*\//) in_block = 1
        next
      }
      n++
    }
    END { print n }
  ' "$1"
}

check_file() {
  local file="$1"
  local lines
  scanned=$((scanned + 1))
  lines="$(effective_lines "$file")"
  if [ "$lines" -gt "$LIMIT" ]; then
    printf '\033[1;31m✖ %s: %s effective lines (limit %s)\033[0m\n' "$file" "$lines" "$LIMIT" >&2
    offenders=$((offenders + 1))
  fi
}

offenders=0
unreadable=0
scanned=0
if [ "$#" -gt 0 ]; then
  while IFS= read -r file; do
    case "$file" in
      *.rs) ;;
      *) continue ;;
    esac
    if [ ! -f "$file" ] || [ ! -r "$file" ]; then
      printf '\033[1;31m✖ %s: named .rs file is unreadable\033[0m\n' "$file" >&2
      unreadable=$((unreadable + 1))
      continue
    fi
    check_file "$file"
  done < <(printf '%s\n' "$@")
else
  while IFS= read -r file; do
    check_file "$file"
  done < <(find "${ROOTS[@]}" -type f -name '*.rs' | sort)
fi

if [ "$unreadable" -ne 0 ]; then
  printf '\033[1;31m✖ file-length check failed: %s named .rs file(s) unreadable — failing closed.\033[0m\n' \
    "$unreadable" >&2
  exit 1
fi

if [ "$offenders" -ne 0 ]; then
  printf '\033[1;31m✖ file-length check failed: %s file(s) over %s effective lines (scanned %s).\033[0m\n' \
    "$offenders" "$LIMIT" "$scanned" >&2
  printf '  Remedy: split the file into subdirectories; no override list exists.\n' >&2
  exit 1
fi

printf '\033[1;32m✔ file-length check: %s files within %s effective lines.\033[0m\n' "$scanned" "$LIMIT"
