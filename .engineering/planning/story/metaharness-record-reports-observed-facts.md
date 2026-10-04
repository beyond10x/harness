---
format: aep.planning-md/3
id: story:metaharness-record-reports-observed-facts
kind: story
status: implemented
title: Metaharness records report the agents, denials, delegation, and usage observed
summary: Conversion no longer hard-codes empty facts contradicted by harness events.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:28Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:28Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:35Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

The converter emits null agents, empty permission denials, zero spawned subagents, and absent cache-creation usage even when Started, approval, delegation, and usage events contain those facts.

## Acceptance

The conversion aggregates source events deterministically and preserves absence only when the source is absent. Tests cover agents, denied approvals, successful and refused delegates, cache-creation usage, nested events, and a no-data case. No field claims zero merely because conversion omitted it.
