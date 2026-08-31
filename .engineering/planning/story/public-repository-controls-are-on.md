---
format: aep.planning-md/1
id: story:public-repository-controls-are-on
kind: story
status: active
title: Public repository controls are on
summary: Reporting, scanning, push protection, dependency updates, and public discovery are configured and verified.
tags:
- governance
- security
relations:
- derived_from: epic:public-site-is-accurate-live-and-governed
- serves: vision:b10x-owns-its-loop
revision: 4
---
# Story: Public repository controls are on

## Outcome

Security reports have a private path and accidental credential/dependency exposure is checked at the public repository boundary.

## Context

Private vulnerability reporting, secret scanning, push protection, Dependabot security updates, and the homepage are currently disabled or unset.

## Acceptance

- Root SECURITY.md directs reports to GitHub's private vulnerability flow and public issues away from secrets.
- Private vulnerability reporting, secret scanning, push protection, and Dependabot security updates report enabled through the API.
- The homepage points to the verified site.
- README/site say public source remains proprietary and grant no open-source licence.

## Out of Scope

No SLA, bug bounty, legal license drafting, or Atlas setting change.

## Open Questions

None.
