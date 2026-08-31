---
format: aep.planning-md/1
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
---
## Defect

The converter emits null agents, empty permission denials, zero spawned subagents, and absent cache-creation usage even when Started, approval, delegation, and usage events contain those facts.

## Acceptance

The conversion aggregates source events deterministically and preserves absence only when the source is absent. Tests cover agents, denied approvals, successful and refused delegates, cache-creation usage, nested events, and a no-data case. No field claims zero merely because conversion omitted it.
