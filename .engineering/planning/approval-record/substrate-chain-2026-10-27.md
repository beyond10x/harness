---
format: aep.planning-md/3
id: approval-record:substrate-chain-2026-10-27
kind: approval-record
status: approved
title: Substrate chain item approved by session b10x-chef-orchestrator
summary: 'D80 and D81, 2026-10-27: newest Substrate in harness, metaharness, loom and takt'
relations:
- decides: story:substrate-pin-0-12
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-10-27T08:58:29Z", actor: "human:timo", revision: 2}
---
## The stop

Chain item D80 (2026-10-27): move harness, metaharness, loom and takt to the newest Substrate
release, developed against local checkouts with path overrides, released in order harness ->
metaharness -> loom, each pin-only on the previous tag; takt lands on its integration branch only.
Amended by D81 the same day: the substrate path override points at Substrate's ADR 0109 follow-up
branch, and the releases pin the follow-up release, not 0.12.0.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-27, under the operator's standing grant for class-C items
in organization repositories. A relayed approval, not a bypass. The operator's words, as relayed:

> yes, we want to upgrade to latest substrate for: loom, takt, harness, metaharness.

> Substrate should be used for Loom and Takt whenever we are using sandboxes

> sure, then start early, but do not forget it later

## Decisions the coordinator took inside the item

- The pins name the Substrate tag commit and an exact `=` version, as every Substrate pin here did.
- The specification moves to the newest ESS release (0.70.0) in the same change, per the
  workspace rule.
