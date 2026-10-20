---
format: aep.planning-md/3
id: approval-record:wave-tools-1-proposal
kind: approval-record
status: draft
title: Wave tools-1 approved by session adr-changes under the owner's exception 5
tags:
- adr-changes
relations:
- decides: story:harness-consumes-tools-crates
- decides: epic:harness-on-llm-and-tools
revision: 1
---
## The stop

Stage-1 proposal of wave tools-1 (`docs/plans/2026-10-20-wave-tools-1.md`): one unit,
`story:harness-consumes-tools-crates`, Atlas ADR 0097 decision 3 / ADR 0095 phase T2.

## Who decided

Session `adr-changes`, 2026-10-20, under the owner's standing exception 5. The owner was away; a
relayed approval, not a bypass. Verbatim: "APPROVED under exception 5: wave tools-1 as written in
docs/plans/2026-10-20-wave-tools-1.md (U1 story:harness-consumes-tools-crates, the name mapping as
the scoper settled it, model-facing names unchanged, opening/unit/merge/closing commits, gate,
fast-forward main through the bot route). Start it after 0.14.1 is released. Release 0.15.0 at the
close is approved too (metaharness pins it)."

## What it authorises

The commits the wave page § *The commits approval would authorise* lists, and release 0.15.0 after
the merge. The route is wave llm-1's, corrected by the identity ruleset: local bot commits, a
neutrally-named branch pushed for the required checks, fast-forward of `main` through the bot once
the checks are green on the commit, no revealing branch name. Harness keeps its CLI contract tool
names toward the model and metaharness; the adapter maps them to the tools operation ids; no new
CLI contract version. 0.14.1 is released (tag `f109612e`), so the wave may start.
