---
format: aep.planning-md/1
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
---
## Defect

Compaction replaces ranges containing opaque provider items, Responses filters unknown output content, and both stream decoders skip unmodelled events or content deltas. The next turn then observes a conversation with holes.

## Acceptance

Every unmodelled event or output item is preserved with its producing wire identity and emits a warning. Compaction never destroys opaque items. Replay on the producing wire is verbatim; replay on another wire is a typed refusal. Contract fixtures cover unknown events, unknown content, compaction, and cross-wire replay.
