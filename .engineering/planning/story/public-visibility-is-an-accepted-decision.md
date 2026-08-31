---
format: aep.planning-md/1
id: story:public-visibility-is-an-accepted-decision
kind: story
status: implemented
title: Public visibility is an accepted decision
summary: Atlas and Harness agree that Harness is public, proprietary, and not a stability claim.
tags:
- governance
- security
relations:
- derived_from: epic:public-site-is-accurate-live-and-governed
- serves: vision:b10x-owns-its-loop
revision: 5
---
# Story: Public visibility is an accepted decision

## Outcome

Readers and maintainers see one truthful decision: Harness is public source under a proprietary licence, while Atlas remains private.

## Context

GitHub reports Harness public; Harness and the private Atlas map say private, and only Substrate currently has an accepted visibility ADR.

## Acceptance

- A private Atlas ADR records the owner decision, observed prior exposure, secret-scan evidence, proprietary posture, and order of moves.
- Atlas's private map and log agree; Atlas remains private and no public page links it.
- Harness AGENTS and README agree with the ADR and make no open-source or stability claim.
- Exact suppressions cover only four reviewed Gitleaks fingerprints and the final history scan is green.

## Out of Scope

No new license terms or relicensing.

## Open Questions

None.
