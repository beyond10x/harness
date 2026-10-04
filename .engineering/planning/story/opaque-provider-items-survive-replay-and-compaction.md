---
format: aep.planning-md/3
id: story:opaque-provider-items-survive-replay-and-compaction
kind: story
status: implemented
title: Opaque provider state survives streaming, replay, and compaction
summary: Unknown events and content remain byte-preserving items and are never silently dropped.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:27Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:27Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:35Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

Compaction replaces ranges containing opaque provider items, Responses filters unknown output content, and both stream decoders skip unmodelled events or content deltas. The next turn then observes a conversation with holes.

## Acceptance

Every unmodelled event or output item is preserved with its producing wire identity and emits a warning. Compaction never destroys opaque items. Replay on the producing wire is verbatim; replay on another wire is a typed refusal. Contract fixtures cover unknown events, unknown content, compaction, and cross-wire replay.
