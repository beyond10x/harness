#!/usr/bin/env bash
# Rehearse the compaction measurement at several declared windows and print a couple of lines each.
#
# Why a sweep and not one run: the paid run declares one window and gets one answer, and there is
# no way to tell from one answer whether the figure is a property of the loop or of the window that
# happened to be picked. The sweep is free, so the paid run can be aimed at a window whose
# behaviour is already understood instead of discovering it at the vendor's expense.
#
# Free. Every line it prints is `provider_emulated` and stays that way.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
windows=("$@")
[ ${#windows[@]} -gt 0 ] || windows=(16000 8000 4000 2500)

for window in "${windows[@]}"; do
  echo "=== --context-window $window"
  bash "$root/measurements/dry-run-compaction.sh" --context-window "$window" >/dev/null 2>&1
  "$root/measurements/score.py" compaction \
    "$root/measurements/runs/m1-dry-compaction-$window.jsonl" \
    --context-window "$window" --expect-keys --brief
done
