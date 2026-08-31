---
format: aep.planning-md/1
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
---
## Defect

Neighbouring delegates run against forked catalogues whose mutable state is merged last-writer-wins. Conflicting effectful calls can therefore produce a final state different from the required sequential fallback.

## Acceptance

Parallel execution occurs only when every reachable entry is proven safe for concurrent observation; otherwise the delegates run in order. Tests use conflicting writes and catalogue mutations to compare forced-parallel eligibility, automatic fallback, event order, outcomes, and final state. Delegation widens no grant.
