---
format: aep.planning-md/1
id: epic:llm-adoption
kind: epic
status: draft
title: Harness adopts released LLM interfaces without moving execution authority
revision: 1
---
## Outcome

Harness adopts released LLM interfaces without moving execution authority.

## Prerequisite

An exact qualified LLM release is required. No runtime code changes as part of this planning delivery. The new domain is owned by llm/spec/system.yaml and its declaration-domain specification; no duplicate entity model is introduced here.

## Evidence

Operator direction 2026-09-19; README.md; AGENTS.md; LLM docs/design.md. Atlas initiative:llm-foundation coordinates the order.

## External prerequisites

- depends_on:llm/story:foundation-qualified
- informed_by:llm/specification:declaration-domain

AEP 0.55.0 currently refuses cross-member targets while creating mutation locator evidence (kind contains disallowed character /). These are explicit references, not admitted graph edges. The local llm-foundation-release blocker gates adoption until the exact upstream release evidence exists.
