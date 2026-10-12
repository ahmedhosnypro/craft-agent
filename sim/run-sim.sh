#!/usr/bin/env bash
# Host-side orchestrator for the runner simulation: build the release,
# build the runner image, clone a fresh workspace, run one mission in
# the container. Secrets arrive as runtime env only — never baked into
# the image, never echoed.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image="craft-agent-sim:sim"
task="${1:-open-pr-01}"
triple="x86_64-unknown-linux-musl"
repo_url="https://github.com/ahmedhosnypro/craft-agent.git"
env_file="$repo_root/.env"
provider="${CRAFT_AGENT_PROVIDER:-openai}"
model="${CRAFT_AGENT_MODEL:-glm-5.3-flash}"

fail() {
  printf 'run-sim: %s\n' "$1" >&2
  exit 1
}

# The release build runs through the rustup proxies so the pinned
# toolchain and its musl std apply; a distro cargo shadows them.
export PATH="$HOME/.cargo/bin:$PATH"

[ -f "$env_file" ] || fail "missing ${env_file} — the gateway knobs live there"
command -v gh >/dev/null || fail "gh is required on the host for the clone"
command -v docker >/dev/null || fail "docker is required on the host"

set -a
. "$env_file"
set +a
[ -n "${TEST_BYOK_API_KEY:-}" ] || fail "TEST_BYOK_API_KEY is empty in .env"
[ -n "${TEST_BYOK_BASE_URL:-}" ] || fail "TEST_BYOK_BASE_URL is empty in .env"

[ -n "${GH_TOKEN:-}" ] || fail "GH_TOKEN is not set — add it to .env or export it"
login="$(GH_TOKEN="$GH_TOKEN" gh api user --jq .login 2>/dev/null || true)"
[ -n "$login" ] || fail "GitHub rejected GH_TOKEN — refresh it in .env or the environment"
GH_TOKEN="$GH_TOKEN" gh repo view ahmedhosnypro/craft-agent >/dev/null 2>&1 \
  || fail "GH_TOKEN cannot read ahmedhosnypro/craft-agent"
OPENAI_API_KEY="$TEST_BYOK_API_KEY"
OPENAI_BASE_URL="$TEST_BYOK_BASE_URL"
export GH_TOKEN OPENAI_API_KEY OPENAI_BASE_URL

# Build and run share the host network: the container must reach the
# gateway on the host loopback, and this Docker Desktop host's default
# bridge cannot resolve or route.
"$repo_root/scripts/build-release.sh" "$triple"
docker build --network host -t "$image" "$repo_root/sim"

image_id="$(docker image inspect -f '{{.Id}}' "$image")"
checksums="$repo_root/dist/checksums.txt"
[ -f "$checksums" ] || fail "build produced no ${checksums}"
agent_sha="$(awk -v asset="craft-agent-${triple}" '$2 == asset {print $1; exit}' "$checksums")"
helpers_sha="$(awk -v asset="craft-helpers-${triple}" '$2 == asset {print $1; exit}' "$checksums")"
[ -n "$agent_sha" ] || fail "no checksums.txt entry for craft-agent-${triple}"
[ -n "$helpers_sha" ] || fail "no checksums.txt entry for craft-helpers-${triple}"

base="$repo_root/sim/runs/$(date +%Y%m%d-%H%M%S)"
run_dir="$base"
n=0
# mkdir is the lock: concurrent launches in the same second get a suffix.
while ! mkdir "$run_dir" 2>/dev/null; do
  n=$((n + 1))
  run_dir="${base}-${n}"
done
mkdir -p "$run_dir/workspace" "$run_dir/out" "$run_dir/tmp"

# The manifest describes the run host-side; no key or token ever lands in it.
manifest="$run_dir/manifest.json"

update_manifest() {
  local filter="$1"; shift
  jq "$@" "$filter" "$manifest" > "${manifest}.tmp" &&
    mv "${manifest}.tmp" "$manifest"
}

knob_number() {
  jq -n --arg value "$1" 'try ($value | tonumber) catch $value'
}

started_at="$(date +%Y-%m-%dT%H:%M:%S%:z)"
jq -n \
  --arg status "cloning" \
  --arg task "$task" \
  --arg mission "run" \
  --arg repo "$repo_url" \
  --arg image "$image" \
  --arg image_id "$image_id" \
  --arg agent_sha "$agent_sha" \
  --arg helpers_sha "$helpers_sha" \
  --argjson max_turns "$(knob_number "${CRAFT_AGENT_MAX_TURNS:-40}")" \
  --argjson max_tokens "$(knob_number "${CRAFT_AGENT_MAX_TOKENS:-4000000}")" \
  --arg provider "$provider" \
  --arg model "$model" \
  --arg started_at "$started_at" \
  '{schema: 1,
    status: $status,
    task: $task,
    mission: $mission,
    repo: $repo,
    clone_commit: null,
    image: $image,
    image_id: $image_id,
    binaries: {"craft-agent": $agent_sha, "craft-helpers": $helpers_sha},
    knobs: {max_turns: $max_turns, max_tokens: $max_tokens,
            provider: $provider, model: $model},
    started_at: $started_at,
    finished_at: null,
    wall_seconds: null,
    exit_code: null,
    conclusion: null}' > "${manifest}.tmp" &&
  mv "${manifest}.tmp" "$manifest"

# The clone targets this repository only; the credential helper rides
# GH_TOKEN, so the token never lands in a URL or a config file. A failed
# clone marks the manifest and keeps the run dir as evidence.
if ! git -c credential.helper='!gh auth git-credential' clone "$repo_url" \
  "$run_dir/workspace"; then
  update_manifest '.status = $status | .finished_at = $finished_at' \
    --arg status "clone-failed" \
    --arg finished_at "$(date +%Y-%m-%dT%H:%M:%S%:z)"
  fail "clone of ${repo_url} failed — run dir kept: ${run_dir}"
fi

update_manifest '.clone_commit = $sha' \
  --arg sha "$(git -C "$run_dir/workspace" rev-parse HEAD)"

SECONDS=0
rc=0
docker run --rm --network host \
  -e GH_TOKEN \
  -e CRAFT_AGENT_PROVIDER="$provider" \
  -e CRAFT_AGENT_MODEL="$model" \
  -e OPENAI_API_KEY \
  -e OPENAI_BASE_URL \
  -e CRAFT_AGENT_MAX_TURNS="${CRAFT_AGENT_MAX_TURNS:-40}" \
  -e CRAFT_AGENT_MAX_TOKENS="${CRAFT_AGENT_MAX_TOKENS:-4000000}" \
  -e CRAFT_AGENT_STEP_RETRIES \
  -e CRAFT_AGENT_RESULT_WINDOW_TURNS \
  -e CRAFT_AGENT_EGRESS_ALLOWLIST \
  -e CRAFT_AGENT_WRITE_REPO \
  -e CRAFT_AGENT_PROMPT_FILE="/tasks/${task}.md" \
  -e SIM_HOST_UID="$(id -u)" \
  -e SIM_HOST_GID="$(id -g)" \
  -v "$repo_root/dist":/dist:ro \
  -v "$run_dir/workspace":/workspace \
  -v "$run_dir/out":/out \
  -v "$run_dir/tmp":/runner/temp \
  -v "$repo_root/sim/tasks":/tasks:ro \
  "$image" 2>&1 | tee "$run_dir/container.log" || rc=$?

outputs="$run_dir/out/outputs.env"
conclusion=""
if [ -f "$outputs" ]; then
  conclusion="$(sed -n 's/^conclusion=//p' "$outputs" | head -n 1)"
  exit_line="$(sed -n 's/^exit-code=//p' "$outputs" | head -n 1)"
  stream_line="$(sed -n 's/^stream=//p' "$outputs" | head -n 1)"
  final_message="$(awk '
    /^final-message<<CRAFT_AGENT_EOF$/ { flag = 1; next }
    /^CRAFT_AGENT_EOF$/ { flag = 0 }
    flag' "$outputs")"
  printf 'conclusion: %s\n' "${conclusion:-none}"
  printf 'exit-code: %s\n' "${exit_line:-$rc}"
  printf 'stream: %s\n' "${stream_line:-none}"
  printf 'final-message: %s\n' "${final_message:-(empty)}"
else
  printf 'conclusion: none — the container produced no outputs.env\n'
  printf 'exit-code: %s\n' "$rc"
fi

update_manifest \
  '.status = "completed" |
   .finished_at = $finished_at |
   .wall_seconds = $wall_seconds |
   .exit_code = $exit_code |
   .conclusion = (if $conclusion == "" then null else $conclusion end)' \
  --arg finished_at "$(date +%Y-%m-%dT%H:%M:%S%:z)" \
  --argjson wall_seconds "$SECONDS" \
  --argjson exit_code "$rc" \
  --arg conclusion "$conclusion"
printf 'run dir: %s\n' "$run_dir"
exit "$rc"
