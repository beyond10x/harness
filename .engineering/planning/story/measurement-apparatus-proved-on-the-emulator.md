---
format: aep.planning-md/1
id: story:measurement-apparatus-proved-on-the-emulator
kind: story
status: implemented
title: The measurement apparatus is proved against the emulator before it meets a provider
summary: A fixture, a dated rate card and a scorer that produce the compaction and surface figures from a free deterministic run.
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Outcome
The compaction and surface measurements can be produced end to end without a provider, so the paid run is one confirmation rather than several exploratory attempts.

## What it caught, before any money moved
- The rehearsal the preparation document recommended — the shipped `flat-tool` scenario at `--context-window 50` — emits **no `compacted` event at all**. `compact_run` returns silently when nothing was elided, so a scorer cannot tell "never fired" from "fired and freed nothing". The cause is `elide`'s unconditional one-result floor plus a scenario that makes exactly one call, so `elidable` is 0 at any declared window. `KEPT_RESULT_BYTES` was not the mechanism; `protected_bytes` already makes it non-binding at a small window.
- Enforceability is decided from the model the run **asked for**; each turn is priced against the model the provider **reported**. When the two differ the run starts, spends, and then stops with `budget-unobservable`. A single-keyed rate card would have bought a run that aborted mid-flight after spending.

## Three findings the paid run should know
- The trigger overshoots by a whole turn: occupancy is checked between turns, so 90.3% at a 16000 window, 105–119% at 4000, 168–190% at 2500. `COMPACTION_TRIGGER_PERCENT = 80` is a floor, not the trigger.
- A declared window below roughly twice one tool result cannot be reached: the single newest result, which `elide` never touches, already exceeds the target.
- `summary_turn` fired at no window tried. On a read-heavy conversation elision alone reached the target every time, so the paid run will measure the trigger and the ratio and leave the summary prompt untouched — which is an operator decision before spending, not after.

## Evidence class
Everything here is `provider_emulated` and the scorer stamps that field itself. A rehearsal against the emulator is not the measurement and never becomes one (invariant 18). This story does not close `story:compaction-measured-live` or `story:flat-surface-measured`.
