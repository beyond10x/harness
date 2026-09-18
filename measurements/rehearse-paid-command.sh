#!/usr/bin/env bash
# Run the **exact** paid Tier A command, with the endpoint swapped for the deterministic local one.
#
# Free. Nothing outside this machine is contacted and no credential is read: the provider's endpoint
# and credential source are both replaced by a throwaway `[providers.claude]` table, and the file it
# points at contains the string `emulator-only-not-a-credential`.
#
# Everything else is byte-identical to the command an operator would authorize -- the same
# `--model haiku`, the same declared window, the same fixture, the same dated rate card, the same
# ceiling. What it proves is that the command parses, that `haiku` resolves to
# `claude-haiku-4-5-20251001`, that the card is accepted for that identifier and the spend ceiling
# is therefore enforceable, and that the run starts.
#
# **It is expected to end in `BudgetUnobservable`**, and that is the point of running it. The
# emulator answers as `b10x-emulated`, which the Haiku card does not price; enforceability is
# decided from the model the run *asked for*, but each turn is priced against the model the provider
# *reported*, so a run whose served model the card does not cover starts, spends, and stops. On the
# vendor route the served model is the one the card names and the run goes on. Seeing this here for
# nothing is why the card prices both the dated identifier and the family one.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="${B10X_HARNESS:-$root/target/debug/b10x-harness}"
scratch="$(mktemp -d)"
announce="$(mktemp)"
trap 'kill "${emulator:-}" 2>/dev/null || true; rm -rf "$scratch" "$announce"' EXIT

python3 "$root/crates/harness-messages/tests/fixtures/fake_messages.py" \
  --scenario compaction >"$announce" 2>/dev/null &
emulator=$!
for _ in $(seq 1 100); do [ -s "$announce" ] && break; sleep 0.1; done
base_url="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["base_url"])' "$announce")"

printf '%s' '{"claudeAiOauth": {"accessToken": "emulator-only-not-a-credential"}}' >"$scratch/token"
mkdir -p "$scratch/config/b10x"
cat >"$scratch/config/b10x/harness.toml" <<TOML
[default]
provider = "claude"

[providers.claude]
base-url = "$base_url"
oauth-token-file = "$scratch/token"
TOML
export XDG_CONFIG_HOME="$scratch/config"

python3 "$root/measurements/fixtures/make-compaction-workspace.py" "$scratch/fixture" >&2

"$binary" run \
  --model haiku \
  --context-window 16000 \
  --workspace "$scratch/fixture" \
  --input "$(cat "$root/measurements/prompts/compaction-task.txt")" \
  --prices "$root/measurements/rates/claude-haiku-4-5-2026-09-18.json" \
  --max-cost-microunits 250000 \
  --max-turns 24 \
  --session-dir "$scratch/sessions" \
  --json >"$scratch/out.jsonl" 2>"$scratch/err.txt" || true

sed "s#$scratch#<scratch>#g" "$scratch/err.txt" | head -3
python3 - "$scratch/out.jsonl" <<'PY'
import json, sys
for line in open(sys.argv[1]):
    event = json.loads(line)
    if event["kind"] == "started":
        print("started.model :", event["model"], " <- the key the rate card has to carry")
    elif event["kind"] == "rates":
        print("rates.as_of   :", event["as_of"])
    elif event["kind"] == "refused":
        print("refused       :", event["reason"])
    elif event["kind"] == "compacted":
        print("compacted     :", json.dumps(event))
    elif event["kind"] == "finished":
        print("terminal      :", json.dumps(event["stop"]))
PY
