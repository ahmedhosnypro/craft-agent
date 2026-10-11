#!/usr/bin/env bash
# Agent-file validator. Lints every agent definition under
# .craft-agent/agents/: flat YAML frontmatter with known keys only (a
# typo'd field fails loud), required name/description, name charset and
# file-stem coherence, description length/placeholder/trigger rules, a
# present bounded body, and registry-bounded tools/model values.
#
# Modes:
#   (no argument)  scan AGENTS_DIR. A missing dir or a dir with no
#                  agent files is a printed skip (exit 0); invalid
#                  present files fail the gate.
#   --selftest     prove every rule on the canary fixtures — each rule
#                  fires on its labeled bad canary, nothing fires on
#                  the good canary, the fail-closed probes hold, and
#                  the tree scan passes clean.
#   <path>...      per-file checker mode (iterate on one agent file).
#
# Exit codes: 0 = clean, 1 = findings OR a broken scan (a scan that
# cannot run must never pass silently), 2 = usage error.
set -euo pipefail
cd "$(dirname "$0")/.."

usage() {
  printf 'usage: %s [--selftest | <agent-file>...]\n' "$0" >&2
  exit 2
}

case "${1:-}" in
  "") scan_mode=tree ;;
  --selftest) scan_mode=selftest ;;
  -*) usage ;;
  *) scan_mode=paths ;;
esac
if [ "$scan_mode" = "selftest" ] && [ "$#" -gt 1 ]; then
  usage
fi

AGENTS_DIR=".craft-agent/agents"
FIXTURE_DIR="scripts/fixtures/agent-canaries"

REQUIRED_FIELDS=(name description)
KNOWN_FIELDS=(
  name
  description
  model
  promptMode
  tools
  disallowedTools
  maxTurns
  timeoutSecs
  background
  agentsMd
)
TOOLS_FIELDS=(tools disallowedTools)
REGISTRY=(bash read_file write_file edit_file glob grep gh)
# Enumerated character classes keep the matches locale-independent.
NAME_RE='^[abcdefghijklmnopqrstuvwxyz][abcdefghijklmnopqrstuvwxyz0123456789-]{0,63}$'
MODEL_RE='^[abcdefghijklmnopqrstuvwxyz0123456789][abcdefghijklmnopqrstuvwxyz0123456789._/-]*$'
PLACEHOLDERS=(todo tbd wip)
TRIGGER_WORDS=(use when run for against)
DESC_BYTE_CAP=1024
BODY_LINE_LIMIT=300
FILE_BYTE_CAP=65536

KNOWN_SPACED="${KNOWN_FIELDS[*]}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM

# fail_rule <file> <line> <rule> <label> <message>
fail_rule() {
  printf '%s:%s: %s %s: %s\n' "$1" "$2" "$3" "$4" "$5" >&2
}

# effective_lines <file> — identical line accounting to the file-length
# gate, so the two gates can never disagree on what a line is.
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

# validate_file <path> — one agent file; findings to stderr, rc 0/1.
validate_file() {
  local file="$1"
  if [ ! -f "$file" ] || [ ! -r "$file" ]; then
    printf 'agents validate: cannot read %s (failing closed)\n' "$file" >&2
    return 1
  fi
  local found=0 size first close prc=0
  size="$(wc -c < "$file")"
  if [ "$size" -gt "$FILE_BYTE_CAP" ]; then
    fail_rule "$file" 1 MD02 file-size "file is ${size} bytes (cap ${FILE_BYTE_CAP})"
    found=1
  fi
  # Read the first line bounded: a hostile file may hold one huge line.
  first="$(head -c 4 -- "$file")"
  if [ "$first" != "---" ] && [ "$first" != $'---\r' ]; then
    fail_rule "$file" 1 FM01 frontmatter "file must start with a --- line"
    return 1
  fi
  close="$(grep -nE $'^---\r?$' -- "$file" | sed -n '2p' | cut -d: -f1)"
  if [ -z "$close" ]; then
    fail_rule "$file" 1 FM01 frontmatter "no closing --- line"
    return 1
  fi
  if [ "$close" -le 2 ]; then
    fail_rule "$file" 1 FM01 frontmatter "frontmatter block is empty"
    found=1
  fi
  awk -v closeline="$close" -v known="$KNOWN_SPACED" '
    function emitv(i, t, v) {
      v = ""
      for (i = 1; i <= vparts; i++) {
        t = parts[i]
        gsub(/^[ \t]+|[ \t]+$/, "", t)
        if (t == "") continue
        if (v == "") v = t
        else v = v " " t
      }
      printf "V\t%d\t%s\t%s\n", curline, cur, v
    }
    BEGIN { cur = ""; vparts = 0 }
    NR == 1 { next }
    NR >= closeline { next }
    {
      line = $0
      sub(/\r$/, "", line)
      if (index(line, "\t") > 0) {
        printf "F\t%d\ttab character in frontmatter\n", NR
        next
      }
      if (line ~ /^[A-Za-z][A-Za-z0-9_]*:/) {
        if (cur != "" && cur != "__dup__") emitv()
        key = line
        sub(/:.*/, "", key)
        val = line
        sub(/^[A-Za-z][A-Za-z0-9_]*:[ \t]*/, "", val)
        if (key in seen) {
          printf "F\t%d\tduplicate key: %s\n", NR, key
          cur = "__dup__"
          next
        }
        seen[key] = 1
        isknown = 0
        kn = split(known, ka, " ")
        for (i = 1; i <= kn; i++) if (ka[i] == key) isknown = 1
        if (!isknown) printf "F\t%d\tunknown frontmatter key: %s\n", NR, key
        cur = key
        curline = NR
        vparts = 0
        split("", parts)
        if (val == ">" || val == ">-" || val == ">+" || val == "|" || val == "|-" || val == "|+") next
        parts[++vparts] = val
        next
      }
      if (line ~ /^[ \t]*$/) {
        printf "F\t%d\tblank line in frontmatter\n", NR
        next
      }
      if (line ~ /^- /) {
        content = substr(line, 3)
      } else if (line ~ /^ /) {
        content = line
        sub(/^ +/, "", content)
        if (content ~ /^- /) content = substr(content, 3)
      } else {
        printf "F\t%d\tunparseable frontmatter line\n", NR
        next
      }
      if (cur == "") {
        printf "F\t%d\tlist or continuation line with no key\n", NR
        next
      }
      if (cur == "__dup__") next
      parts[++vparts] = content
    }
    END {
      if (cur != "" && cur != "__dup__") emitv()
    }
  ' "$file" > "$work/parse" || prc=$?
  if [ "$prc" -ne 0 ]; then
    printf 'agents validate: frontmatter parse failed on %s (failing closed)\n' "$file" >&2
    return 1
  fi
  local -A values=() lines=()
  local typ fline fkey fval
  while IFS=$'\t' read -r typ fline fkey fval; do
    case "$typ" in
      F)
        fail_rule "$file" "$fline" FM01 frontmatter "$fkey"
        found=1
        ;;
      V)
        values["$fkey"]="$fval"
        lines["$fkey"]="$fline"
        ;;
    esac
  done < "$work/parse"

  local field
  for field in "${REQUIRED_FIELDS[@]}"; do
    if [ -z "${values[$field]+x}" ]; then
      fail_rule "$file" 1 FM02 required-field "missing required field: $field"
      found=1
    elif [ -z "${values[$field]}" ]; then
      fail_rule "$file" "${lines[$field]}" FM02 required-field "$field is empty"
      found=1
    fi
  done

  local name="${values[name]:-}" name_ok=0
  if [ -n "$name" ]; then
    if [[ "$name" =~ $NAME_RE ]] && [ "$name" != *"--"* ] && [ "$name" != *"-" ]; then
      name_ok=1
    else
      fail_rule "$file" "${lines[name]}" FM03 name-charset \
        "name must be a lowercase slug (a-z, 0-9, single inner hyphens, at most 64 chars, no colon)"
      found=1
    fi
    if [ "$name_ok" -eq 1 ]; then
      local stem="${file##*/}"
      stem="${stem%.md}"
      if [ "$name" != "$stem" ]; then
        fail_rule "$file" "${lines[name]}" FM04 name-stem "name '$name' must equal the file stem '$stem'"
        found=1
      fi
    fi
  fi

  local desc="${values[description]:-}" desc_ok=0 dlen lower ph
  if [ -n "${values[description]+x}" ] && [ -n "$desc" ]; then
    desc_ok=1
    dlen="$(printf '%s' "$desc" | wc -c)"
    if [ "$dlen" -gt "$DESC_BYTE_CAP" ]; then
      fail_rule "$file" "${lines[description]}" FM05 description "description is ${dlen} bytes (cap ${DESC_BYTE_CAP})"
      desc_ok=0
      found=1
    fi
    lower="${desc,,}"
    for ph in "${PLACEHOLDERS[@]}"; do
      if [ "$lower" = "$ph" ]; then
        fail_rule "$file" "${lines[description]}" FM05 description "placeholder description"
        desc_ok=0
        found=1
      fi
    done
  fi
  if [ "$desc_ok" -eq 1 ]; then
    local tw hit=0
    for tw in "${TRIGGER_WORDS[@]}"; do
      case "$lower" in
        *"$tw"*) hit=1 ;;
      esac
    done
    if [ "$hit" -eq 0 ]; then
      fail_rule "$file" "${lines[description]}" FM06 description-trigger \
        "description must say when to use the agent (use/when/run/for/against)"
      found=1
    fi
  fi

  if [ -n "${values[model]+x}" ]; then
    local mval="${values[model]}"
    if [ "$mval" != "inherit" ] && ! [[ "$mval" =~ $MODEL_RE ]]; then
      fail_rule "$file" "${lines[model]}" MODEL01 model "model must be inherit or a bare lowercase id"
      found=1
    fi
  fi

  local tf raw inner tok t bad
  for tf in "${TOOLS_FIELDS[@]}"; do
    [ -n "${values[$tf]+x}" ] || continue
    raw="${values[$tf]}"
    inner="$raw"
    if [[ "$raw" == \[*\] ]]; then
      inner="${raw:1:${#raw}-2}"
    elif [[ "$raw" == \[* ]] || [[ "$raw" == *\] ]]; then
      fail_rule "$file" "${lines[$tf]}" TOOLS01 tools "unbalanced bracket list in $tf"
      found=1
      continue
    fi
    if [[ "$inner" =~ (^[[:space:]]*,|,[[:space:]]*$|,[[:space:]]*,) ]]; then
      fail_rule "$file" "${lines[$tf]}" TOOLS01 tools "empty tool id in $tf"
      found=1
      continue
    fi
    local toks=()
    read -ra toks <<< "$(printf '%s' "$inner" | tr ',' ' ')"
    if [ "${#toks[@]}" -eq 0 ]; then
      fail_rule "$file" "${lines[$tf]}" TOOLS01 tools "empty tool list in $tf"
      found=1
      continue
    fi
    for tok in "${toks[@]}"; do
      bad=1
      for t in "${REGISTRY[@]}"; do
        if [ "$tok" = "$t" ]; then
          bad=0
        fi
      done
      if [ "$bad" -eq 1 ]; then
        fail_rule "$file" "${lines[$tf]}" TOOLS01 tools "unknown tool id '$tok' in $tf"
        found=1
      fi
    done
  done

  prc=0
  sed -n "$((close + 1)),\$p" "$file" > "$work/body" || prc=$?
  if [ "$prc" -ne 0 ]; then
    printf 'agents validate: cannot slice %s (failing closed)\n' "$file" >&2
    return 1
  fi
  if ! grep -q '[^[:space:]]' "$work/body"; then
    fail_rule "$file" "$close" MD01 body "no prompt body after the frontmatter"
    found=1
  fi
  local blines
  blines="$(effective_lines "$work/body")"
  if [ "$blines" -gt "$BODY_LINE_LIMIT" ]; then
    fail_rule "$file" "$close" MD02 body-length "${blines} effective body lines (cap ${BODY_LINE_LIMIT})"
    found=1
  fi
  return "$found"
}

# scan_dir <dir> — the gate scan.
scan_dir() {
  local dir="$1"
  if [ ! -d "$dir" ]; then
    printf 'agents validate: no agent dir %s — skipped\n' "$dir"
    return 0
  fi
  local -a agent_files=()
  mapfile -t agent_files < <(find "$dir" -maxdepth 1 -type f -name '*.md' | sort)
  if [ "${#agent_files[@]}" -eq 0 ]; then
    printf 'agents validate: no agent files in %s — skipped\n' "$dir"
    return 0
  fi
  local f found=0
  for f in "${agent_files[@]}"; do
    if ! validate_file "$f"; then
      found=1
    fi
  done
  if [ "$found" -ne 0 ]; then
    printf 'agents validate FAIL: findings above (%s agent files)\n' "${#agent_files[@]}" >&2
    return 1
  fi
  printf 'agents validate OK: 0 findings (%s agent files)\n' "${#agent_files[@]}"
}

# check_paths <path>... — per-file mode: every named file must validate.
check_paths() {
  local f found=0
  for f in "$@"; do
    if ! validate_file "$f"; then
      found=1
    fi
  done
  if [ "$found" -ne 0 ]; then
    printf 'agents validate FAIL: findings above (%s files)\n' "$#" >&2
    return 1
  fi
  printf 'agents validate OK: 0 findings (%s files)\n' "$#"
}

CANARIES=(
  good-researcher.md
  bad-fm01-frontmatter.md
  bad-fm02-missing-description.md
  bad-fm03-name-charset.md
  bad-fm04-name-stem-mismatch.md
  bad-fm05-description-placeholder.md
  bad-fm06-no-trigger-word.md
  bad-md01-no-body.md
  bad-md02-body-long.md
  bad-tools01-unknown-id.md
  bad-model01-bad-model.md
)
EXPECT=(
  "FM01 bad-fm01-frontmatter.md"
  "FM02 bad-fm02-missing-description.md"
  "FM03 bad-fm03-name-charset.md"
  "FM04 bad-fm04-name-stem-mismatch.md"
  "FM05 bad-fm05-description-placeholder.md"
  "FM06 bad-fm06-no-trigger-word.md"
  "MD01 bad-md01-no-body.md"
  "MD02 bad-md02-body-long.md"
  "TOOLS01 bad-tools01-unknown-id.md"
  "MODEL01 bad-model01-bad-model.md"
)

run_selftest() {
  local failed=0 fixture rule entry rc cap="$work/cap"
  if [ ! -d "$FIXTURE_DIR" ]; then
    printf 'agents validate selftest: canary fixtures missing: %s\n' "$FIXTURE_DIR" >&2
    return 1
  fi
  for fixture in "${CANARIES[@]}"; do
    if [ ! -f "$FIXTURE_DIR/$fixture" ]; then
      printf 'agents validate selftest: canary fixture missing: %s\n' "$fixture" >&2
      failed=1
    fi
  done
  for entry in "${EXPECT[@]}"; do
    rule="${entry%% *}"
    fixture="${entry#* }"
    if [ ! -f "$FIXTURE_DIR/$fixture" ]; then
      continue
    fi
    rc=0
    validate_file "$FIXTURE_DIR/$fixture" 2>"$cap" || rc=$?
    if [ "$rc" -eq 0 ] || ! grep -q "$rule" "$cap"; then
      printf 'agents validate selftest: rule %s did not fire on %s\n' "$rule" "$fixture" >&2
      failed=1
    fi
  done
  rc=0
  validate_file "$FIXTURE_DIR/good-researcher.md" 2>"$cap" || rc=$?
  if [ "$rc" -ne 0 ] || [ -s "$cap" ]; then
    printf 'agents validate selftest: the good canary is not clean\n' >&2
    cat "$cap" >&2
    failed=1
  fi
  rc=0
  validate_file "$work/missing-probe.md" 2>/dev/null || rc=$?
  if [ "$rc" -eq 0 ]; then
    printf 'agents validate selftest: an unreadable path did not fail closed\n' >&2
    failed=1
  fi
  if ! scan_dir "$work/no-such-dir" >"$cap" 2>&1 || ! grep -q 'skipped' "$cap"; then
    printf 'agents validate selftest: a missing agents dir must be a printed skip\n' >&2
    failed=1
  fi
  mkdir -p "$work/empty-dir"
  if ! scan_dir "$work/empty-dir" >"$cap" 2>&1 || ! grep -q 'skipped' "$cap"; then
    printf 'agents validate selftest: an empty agents dir must be a printed skip\n' >&2
    failed=1
  fi
  if ! scan_dir "$AGENTS_DIR" >"$cap" 2>&1; then
    printf 'agents validate selftest: the tree scan is not clean\n' >&2
    cat "$cap" >&2
    failed=1
  fi
  if [ "$failed" -ne 0 ]; then
    printf 'agents validate selftest FAILED — a rule is unfalsifiable or over-broad\n' >&2
    return 1
  fi
  printf 'agents validate selftest OK: every rule fires on its canary; good canary clean; fail-closed holds; tree scan clean\n'
}

case "$scan_mode" in
  selftest)
    if ! run_selftest; then
      exit 1
    fi
    ;;
  paths)
    for arg in "$@"; do
      case "$arg" in
        -*) usage ;;
      esac
    done
    if ! check_paths "$@"; then
      exit 1
    fi
    ;;
  *)
    if ! scan_dir "$AGENTS_DIR"; then
      exit 1
    fi
    ;;
esac
