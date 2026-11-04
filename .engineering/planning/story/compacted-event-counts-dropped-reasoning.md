---
format: aep.planning-md/3
id: story:compacted-event-counts-dropped-reasoning
kind: story
status: draft
title: The Compacted event counts the reasoning items a compaction dropped
tags:
- from-finding
relations:
- informed_by: review-result:adversary-1104-u3-pass-1
- serves: vision:b10x-owns-its-loop
revision: 1
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
