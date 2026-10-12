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

# The container runs as root; without this, files written into the
# host-mounted volumes land root-owned. SIM_HOST_UID is the invoking
# user, never a secret: chown the mounts up front, then each artifact
# as it is written.
host_own() {
  [ -n "${SIM_HOST_UID:-}" ] || return 0
  chown "${SIM_HOST_UID}:${SIM_HOST_GID:-$SIM_HOST_UID}" "$1" ||
    printf 'entrypoint: chown %s failed\n' "$1" >&2
  return 0
}

if [ -n "${SIM_HOST_UID:-}" ]; then
  chown -R "${SIM_HOST_UID}:${SIM_HOST_GID:-$SIM_HOST_UID}" \
    "$OUT_DIR" "$RUNNER_TEMP" "$GITHUB_WORKSPACE" ||
    printf 'entrypoint: chown of mounts failed — artifacts may stay container-owned\n' >&2
fi

# Install step: same verification the action performs on the release —
# checksums.txt must name the triple's assets, one check pass over the
# whole file, then each binary lands in a PATH dir.
[ -f "${DIST_DIR}/checksums.txt" ] ||
  fail "no checksums.txt in ${DIST_DIR}; mount the release dist"

recorded_asset() {
  awk -v asset="$1" \
    '$2 == asset || $2 ~ ("/" asset "$") { print $2; exit }' \
    "${DIST_DIR}/checksums.txt"
}

recorded_agent="$(recorded_asset "craft-agent-${TRIPLE}")"
[ -n "$recorded_agent" ] ||
  fail "no checksums.txt entry for craft-agent-${TRIPLE}"
[ -f "${DIST_DIR}/${recorded_agent}" ] ||
  fail "asset ${recorded_agent} missing from ${DIST_DIR}"

recorded_helpers="$(recorded_asset "craft-helpers-${TRIPLE}")"
[ -n "$recorded_helpers" ] ||
  fail "no checksums.txt entry for craft-helpers-${TRIPLE}"
[ -f "${DIST_DIR}/${recorded_helpers}" ] ||
  fail "asset ${recorded_helpers} missing from ${DIST_DIR}"

(
  cd "$DIST_DIR"
  sha256sum --check checksums.txt --ignore-missing
)

install -m 0755 "${DIST_DIR}/${recorded_agent}" "$CRAFT_AGENT_BIN"
install -m 0755 "${DIST_DIR}/${recorded_helpers}" "$CRAFT_HELPERS_BIN"

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
  host_own "$prompt_file"
elif [ -n "${CRAFT_AGENT_PROMPT_FILE:-}" ]; then
  [ -f "$CRAFT_AGENT_PROMPT_FILE" ] ||
    fail "prompt file ${CRAFT_AGENT_PROMPT_FILE} not found"
  prompt_file="$CRAFT_AGENT_PROMPT_FILE"
else
  : > "$prompt_file"
  host_own "$prompt_file"
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
host_own "$stream"

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

digest_path=""
if [ "$mission" = "digest" ] && [ "$conclusion" = "success" ]; then
  candidate="${GITHUB_WORKSPACE}/out/digest.md"
  if [ -f "$candidate" ]; then
    digest_path="$candidate"
    host_own "$candidate"
  else
    printf 'entrypoint: digest exited 0 but produced no %s\n' "$candidate" >&2
    conclusion="error"
  fi
fi

# A model-emitted terminator line must not truncate the heredoc value.
{
  echo "conclusion=${conclusion}"
  echo "exit-code=${rc}"
  echo "digest-path=${digest_path}"
  echo "stream=out/stream.jsonl"
  echo "final-message<<CRAFT_AGENT_EOF"
  printf '%s\n' "$final_message" | sed '/^CRAFT_AGENT_EOF$/d'
  echo "CRAFT_AGENT_EOF"
} > "${OUT_DIR}/outputs.env"
host_own "${OUT_DIR}/outputs.env"

if [ "$rc" -ne 0 ]; then
  printf '%s\n' "--- craft-agent exit code ${rc}; last stream events: ---" >&2
  tail -n 5 "$stream" >&2 || true
  printf '%s\n' "---" >&2
fi
exit "$rc"
