#!/usr/bin/env bash
# The compaction measurement, rehearsed against the deterministic local endpoint.
#
# Free. No credential is read, nothing outside this machine is contacted, and every figure it
# produces is `provider_emulated` and stays that way (AGENTS.md invariant 18). What it proves is
# the *apparatus* -- the fixture, the flags, the event stream and the scorer -- so that the paid
# run is one confirmation rather than several explorations. What it cannot prove is a provider's
# real token accounting or whether a real model continues sensibly from the summary, which are
# exactly the two things the paid run buys.
#
#   ./measurements/dry-run-compaction.sh                     # the built fixture, window 16000
#   ./measurements/dry-run-compaction.sh --scenario flat-tool --context-window 50
#
# `--scenario flat-tool --context-window 50` is the run `PREP-measured-not-emulated.md` § 5.1
# proposed as needing no new code. It is kept reachable because it is the run that showed why it
# is not enough: see `measurements/README.md`, *The trap the rehearsal caught*.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${B10X_HARNESS:-$root/target/debug/b10x-harness}"
scenario="compaction"
window="16000"
workspace="${TMPDIR:-/tmp}/b10x-m1-fixture"
out=""

while [ $# -gt 0 ]; do
  case "$1" in
    --scenario) scenario="$2"; shift 2 ;;
    --context-window) window="$2"; shift 2 ;;
    --workspace) workspace="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$out" ] || out="$root/measurements/runs/m1-dry-$scenario-$window.jsonl"

python3 "$root/measurements/fixtures/make-compaction-workspace.py" "$workspace" >&2

# The emulator announces its address on stdout and then serves until it is killed. Started
# detached with its announcement in a file, because a pipe held open by a still-running server
# keeps every reader of this script's stdout waiting for it.
announce="$(mktemp)"
python3 "$root/crates/harness-messages/tests/fixtures/fake_messages.py" \
  --scenario "$scenario" >"$announce" 2>/dev/null &
emulator=$!
trap 'kill "$emulator" 2>/dev/null || true; rm -f "$announce"' EXIT

for _ in $(seq 1 100); do
  [ -s "$announce" ] && break
  sleep 0.1
done
base_url="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["base_url"])' "$announce")"

mkdir -p "$(dirname "$out")"
"$binary" run \
  --base-url "$base_url" \
  --wire anthropic-messages \
  --model b10x-emulated \
  --context-window "$window" \
  --workspace "$workspace" \
  --input "$(cat "$root/measurements/prompts/compaction-task.txt")" \
  --prices "$root/measurements/rates/b10x-emulated-2026-09-18.json" \
  --max-cost-microunits 250000 \
  --max-turns 24 \
  --no-session \
  --json >"$out"

echo "wrote $out" >&2
