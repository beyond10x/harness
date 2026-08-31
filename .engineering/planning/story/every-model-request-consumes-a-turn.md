---
format: aep.planning-md/1
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
---
## Defect

Child loops report no turn consumption back to the parent and compaction summaries are declared outside the turn count. A run can therefore make more model requests than max_turns promises.

## Acceptance

Every request sent through any model port consumes exactly one unit before launch, including delegate and summary requests. Delegates receive only their share of the parent's remaining total; sequential and parallel fallback cannot exceed it. Tests pin exact-equality, nested delegation, parallel delegates, and summary-triggered boundaries.
