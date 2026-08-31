---
format: aep.planning-md/1
id: story:summary-work-respects-all-budgets
kind: story
status: implemented
title: Compaction summaries respect deadline, cancellation, and total turn budget
summary: Maintenance work cannot cross a bound and then launch another normal request.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The loop checks deadline before deciding to compact, but summary generation can cross it and the next normal request can still start. Summary work is also omitted from max_turns.

## Acceptance

Before every summary request and immediately after it returns, the loop checks cancellation, deadline, token, cost, and total-turn ceilings. A summary that consumes the remaining bound stops with the appropriate LoopStop and no following provider request. Deterministic clock and blocking-model tests pin the sequence.
