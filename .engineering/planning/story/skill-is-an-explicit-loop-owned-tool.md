---
format: aep.planning-md/3
id: story:skill-is-an-explicit-loop-owned-tool
kind: story
status: implemented
title: The skill tool has an explicit governed ownership contract
summary: Code, AGENTS, and design agree on whether skill is loop-owned or catalogue-owned.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:30Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:30Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:37Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

AGENTS states that exactly answer and delegate belong to the loop and adding a third is a design change, while the loop currently publishes and resolves skill itself.

## Acceptance

A recorded component design chooses one boundary. Either skill moves behind a normal narrowed ToolPort, or the loop-owned inventory is explicitly amended with the same approval, budget, replay, and delegation invariants. Tests and operator documentation pin the chosen ownership; no silent third path remains.
