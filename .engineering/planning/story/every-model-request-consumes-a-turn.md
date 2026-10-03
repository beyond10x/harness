---
format: aep.planning-md/3
id: story:every-model-request-consumes-a-turn
kind: story
status: implemented
title: Every parent, delegate, and summary model request consumes max_turns
summary: The turn ceiling is one total budget across the entire run tree.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:26Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:26Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:34Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

Child loops report no turn consumption back to the parent and compaction summaries are declared outside the turn count. A run can therefore make more model requests than max_turns promises.

## Acceptance

Every request sent through any model port consumes exactly one unit before launch, including delegate and summary requests. Delegates receive only their share of the parent's remaining total; sequential and parallel fallback cannot exceed it. Tests pin exact-equality, nested delegation, parallel delegates, and summary-triggered boundaries.
