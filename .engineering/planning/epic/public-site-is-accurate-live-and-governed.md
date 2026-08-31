---
format: aep.planning-md/1
id: epic:public-site-is-accurate-live-and-governed
kind: epic
status: implemented
title: The public Harness site is accurate, live, and governed
summary: Publish a task-oriented, accessible site whose claims and repository posture remain checked.
tags:
- docs
- public
relations:
- informed_by: review-result:harness-public-site-sweep
- serves: vision:b10x-owns-its-loop
revision: 5
---
# Epic: The public Harness site is accurate, live, and governed

## Outcome

A builder can reach the advertised site, follow a safe task path, and rely on public claims that are checked against the released binary and repository posture.

## Why Now

The repository is already public while its governing documents say private, the Pages URL returns 404, and credential/reference prose disagrees with 0.6.0.

## Scope

Atlas visibility decision, Harness public projection, tutorial/how-to/reference information architecture, landing accessibility, Rust drift checks, repository security controls, and Pages deployment.

## Out of Scope

No Harness runtime, wire, library, CLI, contract, license grant, shared cross-site shell, search service, or 0.6.1 release.

## Risks

Public prose can accidentally expose internal authorities; exact reference can drift; enabling Pages before governance is recorded can deepen the mismatch. Rust checks, preserved routes, separate private Atlas work, and ordered rollout hold those risks.

## Done When

All derived stories are implemented with evidence, both repositories pass their gates, the exact merged Harness commit is deployed, public controls are enabled, and the review result is archived.
