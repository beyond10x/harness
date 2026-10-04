---
format: aep.planning-md/3
id: epic:full-review-remediation
kind: epic
status: implemented
title: Every defect found in the 0.5.0 review is closed with proof
summary: A bounded remediation wave for the immutable full-review finding set.
tags:
- remediation
relations:
- derived_from: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:54:37Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:54:37Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:38Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Outcome

Close every finding in review-result:harness-0-5-0-full-review without weakening a repository invariant or silently changing a released contract.

## Wave constraints

Work only in the wave/full-review-remediation worktree. Preserve released contract directories and cut new versions for observable changes. Each child story moves only with a focused regression test; the epic closes only on the full gate plus strict rustdoc, the atlas brand fence, website build, and the adversarial probes named by the review.

## Selection

The wave is comprehensive rather than prioritised: every review finding is selected because the operator explicitly required all defects to be addressed. Security boundaries land before behavioural cleanup so later tests run against the hardened transport and credential surfaces.
