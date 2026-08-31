---
format: aep.planning-md/1
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
---
## Defect

AGENTS states that exactly answer and delegate belong to the loop and adding a third is a design change, while the loop currently publishes and resolves skill itself.

## Acceptance

A recorded component design chooses one boundary. Either skill moves behind a normal narrowed ToolPort, or the loop-owned inventory is explicitly amended with the same approval, budget, replay, and delegation invariants. Tests and operator documentation pin the chosen ownership; no silent third path remains.
