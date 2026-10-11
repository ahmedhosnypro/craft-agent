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

run_dir="$repo_root/sim/runs/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$run_dir/workspace" "$run_dir/out"

# The clone targets this repository only; the credential helper rides
# GH_TOKEN, so the token never lands in a URL or a config file.
git -c credential.helper='!gh auth git-credential' clone "$repo_url" \
  "$run_dir/workspace"

rc=0
docker run --rm --network host \
  -e GH_TOKEN \
  -e CRAFT_AGENT_PROVIDER=openai \
  -e CRAFT_AGENT_MODEL=glm-5.3-flash \
  -e OPENAI_API_KEY \
  -e OPENAI_BASE_URL \
  -e CRAFT_AGENT_MAX_TURNS=40 \
  -e CRAFT_AGENT_MAX_TOKENS=4000000 \
  -e CRAFT_AGENT_PROMPT_FILE="/tasks/${task}.md" \
  -v "$repo_root/dist":/dist:ro \
  -v "$run_dir/workspace":/workspace \
  -v "$run_dir/out":/out \
  -v "$repo_root/sim/tasks":/tasks:ro \
  "$image" 2>&1 | tee "$run_dir/container.log" || rc=$?

outputs="$run_dir/out/outputs.env"
if [ -f "$outputs" ]; then
  conclusion="$(sed -n 's/^conclusion=//p' "$outputs" | head -n 1)"
  final_message="$(awk '
    /^final-message<<CRAFT_AGENT_EOF$/ { flag = 1; next }
    /^CRAFT_AGENT_EOF$/ { flag = 0 }
    flag' "$outputs")"
  printf 'conclusion: %s\n' "${conclusion:-none}"
  printf 'final-message: %s\n' "${final_message:-(empty)}"
else
  printf 'conclusion: none — the container produced no outputs.env\n'
fi
printf 'run dir: %s\n' "$run_dir"
exit "$rc"
