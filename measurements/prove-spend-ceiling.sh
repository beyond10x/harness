#!/usr/bin/env bash
# Prove `--max-cost-microunits` is enforceable for the model the paid run will name -- for free.
#
# The hard blocker in front of the paid run was that no rate card priced the candidate model.
# `Budget::validate` refuses `max_cost_microunits` by name on a run it cannot price
# (`crates/harness-loop/src/budget.rs`), and the command line validates a second time before the
# event record begins (`crates/harness-cli/src/lib.rs`), so the run does not start at all. That is
# a claim about the *run's own* model -- the one `--model` resolved to -- and nothing about it
# needs a provider. So it can be settled here.
#
# The trick, and the only thing about this script that is not obvious: `[providers.claude]` in a
# throwaway config replaces the endpoint and the credential source while leaving the alias table
# alone, so `--model haiku` still expands to `claude-haiku-4-5-20251001` exactly as it would on the
# paid run, and the request goes to the deterministic local endpoint instead of the vendor.
#
# Note what selects the provider, because it is not a flag: `[default] provider = "claude"` in the
# config, or a `[[profiles]]` entry that sets `provider`. `-p/--profile` names a *profile* -- what a
# run may do -- and a profile names a provider. `--profile claude` selects nothing unless a profile
# happens to be called that.
#
#   * No credential is read. The config points the provider at a file this script writes, whose
#     contents are the string `emulator-only-not-a-credential`.
#   * Nothing outside this machine is contacted.
#   * Case C also demonstrates the second half of the rate card's job: the endpoint answers as
#     `b10x-emulated`, which this card does not price, so the ceiling is enforced and **no cost
#     event is emitted at all**. A card that prices no model the run served reports absence, never
#     a zero. On the paid run the served model is the model the card names and the cost appears.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${B10X_HARNESS:-$root/target/debug/b10x-harness}"
haiku="$root/measurements/rates/claude-haiku-4-5-2026-09-18.json"
emulated="$root/measurements/rates/b10x-emulated-2026-09-18.json"

scratch="$(mktemp -d)"
announce="$(mktemp)"
trap 'kill "${emulator:-}" 2>/dev/null || true; rm -rf "$scratch" "$announce"' EXIT

python3 "$root/crates/harness-messages/tests/fixtures/fake_messages.py" \
  --scenario text >"$announce" 2>/dev/null &
emulator=$!
for _ in $(seq 1 100); do [ -s "$announce" ] && break; sleep 0.1; done
base_url="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["base_url"])' "$announce")"

printf '%s' '{"token": "emulator-only-not-a-credential"}' >"$scratch/token"
mkdir -p "$scratch/config/b10x"
cat >"$scratch/config/b10x/harness.toml" <<TOML
[default]
provider = "claude"

[providers.claude]
base-url = "$base_url"
oauth-token-file = "$scratch/token"
oauth-token-pointer = "/token"
TOML
export XDG_CONFIG_HOME="$scratch/config"

echo "resolved route (alias table untouched, endpoint replaced):"
"$binary" providers show claude | sed "s#$scratch#<scratch>#g"
echo

run() {
  local label="$1"; shift
  echo "--- $label"
  set +e
  "$binary" run --model haiku \
    --workspace "$scratch" --input "say hello" --max-turns 1 --no-session --json "$@" \
    >"$scratch/out.jsonl" 2>"$scratch/err.txt"
  local status=$?
  set -e
  echo "exit status: $status"
  sed "s#$scratch#<scratch>#g" "$scratch/err.txt" | head -4
  python3 - "$scratch/out.jsonl" <<'PY'
import json, sys
kinds = []
for line in open(sys.argv[1]):
    event = json.loads(line)
    kinds.append(event["kind"])
    if event["kind"] == "started":
        print("  started.model:", event["model"], "-- what `--model haiku` resolved to")
    if event["kind"] in ("refused", "cost", "rates"):
        print("  event:", json.dumps({k: v for k, v in event.items() if k != "context"})[:200])
print("  cost events:", kinds.count("cost"), " rates events:", kinds.count("rates"))
PY
  echo
}

run "A  cap, no card at all -- the blocker as it stood" --max-cost-microunits 250000
run "B  cap, a card that prices some other model" \
  --prices "$emulated" --max-cost-microunits 250000
run "C  cap, the dated card that prices claude-haiku-4-5-20251001" \
  --prices "$haiku" --max-cost-microunits 250000
