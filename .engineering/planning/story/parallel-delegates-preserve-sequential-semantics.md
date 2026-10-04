---
format: aep.planning-md/3
id: story:parallel-delegates-preserve-sequential-semantics
kind: story
status: implemented
title: Parallel delegates are observationally equivalent to sequential delegates
summary: Catalogue forks cannot hide or reorder stateful tool effects.
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
- {from: "active", to: "implemented", at: "2026-08-31T03:29:35Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":2}}, imported: true}
---
## Defect

Neighbouring delegates run against forked catalogues whose mutable state is merged last-writer-wins. Conflicting effectful calls can therefore produce a final state different from the required sequential fallback.

## Acceptance

Parallel execution occurs only when every reachable entry is proven safe for concurrent observation; otherwise the delegates run in order. Tests use conflicting writes and catalogue mutations to compare forced-parallel eligibility, automatic fallback, event order, outcomes, and final state. Delegation widens no grant.
