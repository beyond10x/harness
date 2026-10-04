---
format: aep.planning-md/3
id: story:compaction-measured-live
kind: story
status: active
title: Compaction is measured against a real provider, not the emulator
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
- depends_on: story:measurement-apparatus-proved-on-the-emulator
scope:
- confidence: inferred
  path: STATUS.md
- confidence: inferred
  path: measurements
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-09-15T17:26:30Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-09-18T01:22:16Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"metric_observation":1}}, imported: true}
---
## Evidence

- `STATUS.md:12` — "Compaction is token-aware given a `context_window`: 80% to fire, 50% to free, and one extra summary turn where eliding tool output cannot reach the target"; next evidence: "measure a compaction summary against a real provider — the trigger, the ratio and the summary prompt are all `provider_emulated`".
- `crates/harness-loop/src/lib.rs:746` and `:752` — `COMPACTION_TRIGGER_PERCENT = 80`, `COMPACTION_TARGET_PERCENT = 50`.
- `crates/harness-loop/src/lib.rs:2088-2093` — the byte rule that applies when no window is known: 192 KiB, "about 50k tokens".
- `crates/harness-loop/src/lib.rs:2102-2120` — `compact_run`, which measures occupancy against the provider's own last reported input count.
- `docs/reviews/2026-08-29-sota-comparison.md:67` — the finding this replaced: "A long run hits the provider's context wall with a hard error; ~60% of a 128k window is never used."

## Context

The 2026-09-18 live Haiku run recorded in STATUS.md's Loop row and measurements/runs/m1-tier-a-2026-09-18.jsonl measured the trigger and freed ratio. That observation does not establish summary-turn quality or cost: the fallback summary branch still needs a live run that reaches it and completes afterwards. The earlier Evidence section records the original gap, not today's coverage. Retain the story as active for the remaining summary measurement rather than relabelling emulator evidence as vendor-live.

## Acceptance

One live run long enough to trigger compaction, with the trigger point, the freed ratio, the summary
turn's cost and whether the run completed afterwards recorded as `vendor_live` evidence — and
`STATUS.md`'s loop row naming that run instead of naming the measurement as pending.
