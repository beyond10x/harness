#!/usr/bin/env bash
# The b10x string is banned at the surface of this repository. Allowed:
# CHANGELOG history, the parked contract fixtures whose wire bytes rename
# together with the agent-side sender, the two wire-visible names those
# fixtures pin (the `b10x_operation_search` tool and the
# `b10x-emulated` model), the bot-App scripts (the b10x-bot GitHub
# App name and its env vars rename only with the App), and this check.
set -euo pipefail
# The former brand, assembled at runtime: a guard that spells the banned string contiguously
# would itself be a hit. `printf` keeps the pattern out of the file while the check still works.
BANNED="$(printf 'daemon%sloom|codewandler' '')"
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
hits=$(git grep -in "${BANNED}" -- \
  ':!CHANGELOG.md' ':!contracts' ':!scripts/check-brand.sh' \
  ':!scripts/as-bot.sh' ':!scripts/bot-token.sh' ':!scripts/check-bot-files.py' \
  | grep -viE 'b10x_operation_search|b10x-emulated' || true)
if test -n "$hits"; then
  printf 'brand check: b10x at the surface:\n%s\n' "$hits" >&2
  exit 1
fi
printf 'brand check: clean\n'
