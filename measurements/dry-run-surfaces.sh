#!/usr/bin/env bash
# Rehearse the flat-versus-verbs comparison against the deterministic local endpoint.
#
# Free. What it proves is the **apparatus** and nothing whatever about the two surfaces: the
# emulator replays a script, so it cannot choose tools the way a model chooses them, and the whole
# question the measurement asks is what a model does differently under each surface. What it does
# prove, for nothing:
#
#   * `--surface flat` publishes the catalogue and `--surface verbs` publishes exactly the three
#     verbs -- readable from `started.published_tools` with no model turn at all;
#   * the scorer classifies discovery calls, counts failures, sums usage, reads the terminal and
#     says whether the arm completed;
#   * the two arms' scratch trees are independent, which is the thing a shared workspace would
#     silently destroy.
#
# The paid form of this is two runs per repeat differing in one flag, N >= 3, against a workspace
# copied fresh per arm per repeat. See measurements/README.md.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${B10X_HARNESS:-$root/target/debug/b10x-harness}"

names='import json,sys; print(*[t["name"] for t in json.load(sys.stdin)["tools"]])'
echo "=== published surface, with no model turn and no endpoint at all"
for surface in flat verbs; do
  printf '%-6s %s\n' "$surface" "$("$binary" tools --surface "$surface" | python3 -c "$names")"
done
echo

arm() {
  local surface="$1" scenario="$2"
  local workspace; workspace="$(mktemp -d)"
  local announce; announce="$(mktemp)"
  printf 'hello harness\n' >"$workspace/README.md"

  python3 "$root/crates/harness-messages/tests/fixtures/fake_messages.py" \
    --scenario "$scenario" >"$announce" 2>/dev/null &
  local emulator=$!
  for _ in $(seq 1 100); do [ -s "$announce" ] && break; sleep 0.1; done
  local base_url; base_url="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["base_url"])' "$announce")"

  local out="$root/measurements/runs/m2-dry-$surface.jsonl"
  mkdir -p "$(dirname "$out")"
  "$binary" run \
    --base-url "$base_url" --wire anthropic-messages --model b10x-emulated \
    --surface "$surface" \
    --workspace "$workspace" \
    --input "$(cat "$root/measurements/prompts/surface-task.txt")" \
    --prices "$root/measurements/rates/b10x-emulated-2026-09-18.json" \
    --max-cost-microunits 150000 \
    --max-turns 20 --no-session --json >"$out" 2>/dev/null || true
  kill "$emulator" 2>/dev/null || true
  rm -f "$announce"
  echo "workspace $workspace" >&2
  "$root/measurements/score.py" surface "$out" --arm "$surface" --brief
}

echo "=== one walk per arm, apparatus only"
arm flat flat-tool
arm verbs tool
