#!/usr/bin/env bash
# The former brand name (and `codewandler`) are banned at the surface of this
# repository. Exempt: CHANGELOG history, the bot-App scripts (scripts/as-bot.sh,
# scripts/bot-token.sh and scripts/check-bot-files.py carry the b10x-bot GitHub
# App's own name and its B10X_BOT_* env vars, functional identifiers that rename
# only together with the App itself), and this check.
# The pinned contract fixtures under contracts/ are NOT exempt: their wire bytes
# were renamed together with the agent-side sender and the manifests re-pinned to
# match, so a reappearance there is a regression rather than a parked contract.
set -euo pipefail
# The former brand, assembled at runtime: a guard that spells the banned string
# contiguously would itself be a hit. `printf` keeps the pattern out of the file
# while the check still works.
BANNED="$(printf 'daemon%sloom|codewandler' '')"
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
# `git grep` exits 1 for "no matches" and greater than 1 for a real failure.
# Collapsing both into `|| true` -- and quoting the pattern so the shell never
# expanded it -- is how a broken check reported this repository clean.
set +e
hits=$(git grep -inE "${BANNED}" -- \
  ':!CHANGELOG.md' ':!scripts/check-brand.sh' \
  ':!scripts/as-bot.sh' ':!scripts/bot-token.sh' ':!scripts/check-bot-files.py')
status=$?
set -e
if test "$status" -gt 1; then
  printf 'brand check: git grep failed with exit %s\n' "$status" >&2
  exit 1
fi
if test -n "$hits"; then
  printf 'brand check: the former brand at the surface:\n%s\n' "$hits" >&2
  exit 1
fi
printf 'brand check: clean\n'
