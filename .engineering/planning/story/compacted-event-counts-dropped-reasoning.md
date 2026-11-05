---
format: aep.planning-md/3
id: story:compacted-event-counts-dropped-reasoning
kind: story
status: implemented
title: The Compacted event counts the reasoning items a compaction dropped
tags:
- from-finding
relations:
- informed_by: review-result:adversary-1104-u3-pass-1
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: crates/harness-cli/src/render.rs
- confidence: cited
  path: crates/harness-loop/src/event.rs
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/tests.rs
- confidence: cited
  path: website/docs/guides/sessions-and-events.md
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T02:19:07Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-05T02:19:07Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-05T04:12:00Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

Found by `review-result:adversary-1104-u3-pass-1` (finding at `crates/harness-loop/src/lib.rs:3239`,
NEEDS-CHANGE, introduced by the wave 2026-11-04 compaction change). A compaction that only drops
reasoning items emits `LoopEvent::Compacted { elided_results: 0, summarised_items: 0,
summary_turn: false, .. }` while 20,062 bytes went (adversary case
`adversary_u3_a_compaction_that_drops_only_reasoning_says_so_in_its_figures`). The
`conversation-compacted` warning names the count; the event, which a machine reading the record
uses, does not.

Left out of the wave because adding a field to the public `LoopEvent::Compacted` variant changes a
public type that consumers match on; that is a decision of its own.

## Outcome

`LoopEvent::Compacted` carries how many reasoning items a compaction dropped, so every byte the
event reports as gone is attributed to elision, a summary or dropped reasoning.

## Acceptance

- The adversary case above asserts a non-zero dropped-reasoning figure and passes.
- CHANGELOG names the variant change.
