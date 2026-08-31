---
format: aep.planning-md/1
id: story:budgets-bind-when-usage-is-unknown
kind: story
status: implemented
title: Token and cost budgets fail closed when usage cannot be accounted
summary: Missing, partial, aliased, or unpriced usage cannot silently disable a configured ceiling.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

Usage can be absent or partial, a provider can report a model alias the configured rate card does not know, cache-creation tokens have no configured price, and exact equality uses a greater-than test. Each can permit a request beyond the declared budget.

## Acceptance

Exact equality stops before another model request. When a configured token or cost ceiling cannot be evaluated from reported usage, the run stops with a named budget outcome rather than treating absence as zero. Model aliases and cache-creation pricing have explicit, tested policy; unreported usage remains absent in records.
