#!/usr/bin/env bash
# The daemonloom string is banned at the surface of this repository. Allowed:
# CHANGELOG history, pinned provenance URLs, the parked contract fixtures whose
# wire bytes rename together with the agent-side sender, and this check.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
hits=$(git grep -in 'daemonloom' -- \
  ':!CHANGELOG.md' ':!contracts' ':!scripts/check-brand.sh' \
  | grep -viE 'github\.com/daemonloom|the daemonloom monorepo|daemonloom_operation_search|daemonloom_agent_harness|daemonloom-emulated' || true)
if test -n "$hits"; then
  printf 'brand check: daemonloom at the surface:\n%s\n' "$hits" >&2
  exit 1
fi
printf 'brand check: clean\n'
