#!/usr/bin/env bash
# The daemonloom string is banned at the surface of this repository. Allowed:
# CHANGELOG history, pinned provenance URLs, the parked contract fixtures whose
# wire bytes rename together with the agent-side sender, the bot-App scripts
# (the daemonloom-bot GitHub App name and its env vars rename only with the
# App), and this check.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
hits=$(git grep -in 'daemonloom' -- \
  ':!CHANGELOG.md' ':!contracts' ':!scripts/check-brand.sh' \
  ':!scripts/as-bot.sh' ':!scripts/bot-token.sh' ':!scripts/check-bot-files.py' \
  | grep -viE 'github\.com/daemonloom|the daemonloom monorepo|daemonloom_operation_search|daemonloom_agent_harness|daemonloom-emulated' || true)
if test -n "$hits"; then
  printf 'brand check: daemonloom at the surface:\n%s\n' "$hits" >&2
  exit 1
fi
printf 'brand check: clean\n'
