---
format: aep.planning-md/1
id: story:compaction-measured-live
kind: story
status: draft
title: Compaction is measured against a real provider, not the emulator
relations:
- derived_from: epic:measured-not-emulated
revision: 2
---
## Evidence

- `STATUS.md:12` — "Compaction is token-aware given a `context_window`: 80% to fire, 50% to free, and one extra summary turn where eliding tool output cannot reach the target"; next evidence: "measure a compaction summary against a real provider — the trigger, the ratio and the summary prompt are all `provider_emulated`".
- `crates/harness-loop/src/lib.rs:746` and `:752` — `COMPACTION_TRIGGER_PERCENT = 80`, `COMPACTION_TARGET_PERCENT = 50`.
- `crates/harness-loop/src/lib.rs:2088-2093` — the byte rule that applies when no window is known: 192 KiB, "about 50k tokens".
- `crates/harness-loop/src/lib.rs:2102-2120` — `compact_run`, which measures occupancy against the provider's own last reported input count.
- `docs/reviews/2026-08-29-sota-comparison.md:67` — the finding this replaced: "A long run hits the provider's context wall with a hard error; ~60% of a 128k window is never used."

## Context

Three numbers decide whether a long run survives: when compaction fires, how far it frees, and
whether the summary turn keeps enough for the model to continue. All three were chosen against an
emulator that reports whatever input count the fixture says, so none of them has been tested against
a provider's real accounting or a real model's ability to continue from the summary.

The failure this guards against is not a crash: it is a run that compacts, continues, and produces
worse work because the summary lost something. Only a live run shows that.

## Acceptance

One live run long enough to trigger compaction, with the trigger point, the freed ratio, the summary
turn's cost and whether the run completed afterwards recorded as `vendor_live` evidence — and
`STATUS.md`'s loop row naming that run instead of naming the measurement as pending.
