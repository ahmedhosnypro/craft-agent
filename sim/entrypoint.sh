#!/usr/bin/env bash
# Runner-replica entrypoint. Mirrors the composite action's steps against
# the runtime mounts instead of a GitHub release download: /dist carries
# the checksummed release assets (read-only), /workspace is the fresh
# clone, /out receives the stream and outputs. Any argument after the
# image name is executed as a direct craft-agent invocation (e.g.
# `docker run craft-agent-sim:sim --version`) after the install step.
set -euo pipefail

RUNNER_TEMP="${RUNNER_TEMP:-/runner/temp}"
GITHUB_WORKSPACE="${GITHUB_WORKSPACE:-/workspace}"
DIST_DIR="${DIST_DIR:-/dist}"
OUT_DIR="${OUT_DIR:-/out}"
TRIPLE="${CRAFT_AGENT_TRIPLE:-x86_64-unknown-linux-musl}"
CRAFT_AGENT_BIN="/usr/local/bin/craft-agent"
CRAFT_HELPERS_BIN="/usr/local/bin/craft-helpers"

fail() {
  printf 'entrypoint: %s\n' "$1" >&2
  exit 1
}

mkdir -p "$RUNNER_TEMP" "$OUT_DIR"

# Install step: same verification the action performs on the release —
# checksums.txt must name the triple's assets, the check must pass, then
# each binary lands in a PATH dir.
install_binary() {
  local tool="$1" dest="$2" asset recorded
  [ -f "${DIST_DIR}/checksums.txt" ] ||
    fail "no checksums.txt in ${DIST_DIR}; mount the release dist"
  asset="${tool}-${TRIPLE}"
  recorded="$(awk -v asset="$asset" \
    '$2 == asset || $2 ~ ("/" asset "$") { print $2; exit }' \
    "${DIST_DIR}/checksums.txt")"
  [ -n "$recorded" ] ||
    fail "no checksums.txt entry for ${asset}"
  [ -f "${DIST_DIR}/${recorded}" ] ||
    fail "asset ${recorded} missing from ${DIST_DIR}"
  (
    cd "$DIST_DIR"
    sha256sum --check checksums.txt --ignore-missing
  )
  install -m 0755 "${DIST_DIR}/${recorded}" "$dest"
}

install_binary craft-agent "$CRAFT_AGENT_BIN"
install_binary craft-helpers "$CRAFT_HELPERS_BIN"

# Direct binary invocation: hand over to the installed binary.
if [ "$#" -gt 0 ]; then
  exec "$CRAFT_AGENT_BIN" "$@"
fi

# Git setup: credential helper over the runtime GH_TOKEN, the workspace
# marked safe, and the commit identity the agent's pushes carry.
if [ -n "${GH_TOKEN:-}" ]; then
  gh auth setup-git
else
  printf 'entrypoint: GH_TOKEN not set — git pushes will not authenticate\n' >&2
fi
git config --global --add safe.directory "$GITHUB_WORKSPACE"
git config --global user.name "craft-agent"
git config --global user.email "agent@craft-agent.invalid"

# Mission step: the same input mapping the action performs. CRAFT_AGENT_PROMPT
# wins when set; otherwise CRAFT_AGENT_PROMPT_FILE names the source (the
# /tasks mount); with neither, an empty prompt file stands in so the
# binary's own typed validation reports, not a pre-flight harness error.
mission="${CRAFT_AGENT_MISSION:-run}"
case "$mission" in
  run | digest) ;;
  *) fail "unknown mission '${mission}' — expected run or digest" ;;
esac

prompt_file="${RUNNER_TEMP}/prompt.txt"
if [ -n "${CRAFT_AGENT_PROMPT:-}" ]; then
  printf '%s' "$CRAFT_AGENT_PROMPT" > "$prompt_file"
elif [ -n "${CRAFT_AGENT_PROMPT_FILE:-}" ]; then
  [ -f "$CRAFT_AGENT_PROMPT_FILE" ] ||
    fail "prompt file ${CRAFT_AGENT_PROMPT_FILE} not found"
  prompt_file="$CRAFT_AGENT_PROMPT_FILE"
else
  : > "$prompt_file"
fi

# The command is assembled exactly like the action's: run carries
# stream-json and the prompt file; digest takes neither.
cmd=(craft-agent "$mission" --max-turns "${CRAFT_AGENT_MAX_TURNS:-40}"
  --max-tokens "${CRAFT_AGENT_MAX_TOKENS:-4000000}")
if [ "$mission" = "run" ]; then
  cmd+=(--output-format stream-json)
  cmd+=(--prompt "$prompt_file")
fi
if [ -n "${CRAFT_AGENT_ARGS:-}" ]; then
  read -ra extra_args <<< "$CRAFT_AGENT_ARGS"
  cmd+=("${extra_args[@]}")
fi

stream="${OUT_DIR}/stream.jsonl"
cd "$GITHUB_WORKSPACE"
set +e
"${cmd[@]}" > "$stream"
rc=$?
set -e

case "$rc" in
  0) conclusion="success" ;;
  2) conclusion="max-turns" ;;
  3) conclusion="budget" ;;
  4) conclusion="denied" ;;
  *) conclusion="error" ;;
esac

final_message=""
if [ -s "$stream" ]; then
  final_message="$(tail -n 1 "$stream" |
    jq -r 'select(.type == "run_finished") | .detail // ""' 2>/dev/null || true)"
  : "${final_message:=}"
fi

{
  echo "conclusion=${conclusion}"
  echo "final-message<<CRAFT_AGENT_EOF"
  printf '%s\n' "$final_message"
  echo "CRAFT_AGENT_EOF"
} > "${OUT_DIR}/outputs.env"

if [ "$rc" -ne 0 ]; then
  printf '%s\n' "--- craft-agent exit code ${rc}; last stream events: ---" >&2
  tail -n 5 "$stream" >&2 || true
  printf '%s\n' "---" >&2
fi
exit "$rc"
