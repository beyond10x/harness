---
format: aep.planning-md/3
id: approval-record:wave-llm-1-proposal
kind: approval-record
status: draft
title: Wave llm-1 approved by session adr-changes under the owner's exception 5
tags:
- adr-changes
relations:
- decides: story:harness-builds-on-llm-0-7
- decides: epic:harness-on-llm-and-tools
revision: 1
---
## The stop

Stage-1 proposal of wave llm-1 (`docs/plans/2026-10-20-wave-llm-1.md`): one unit,
`story:harness-builds-on-llm-0-7`, on integration branch `wave/2026-10-20-w01`.

## Who decided

Session `adr-changes`, 2026-10-20, under the owner's standing exception 5 for work implementing
Atlas ADRs 0096-0101 (owner's words, 2026-10-20: "I go to bed now, handle all decisions for me").
The owner was not present; this is a relayed approval, not a bypass. Verbatim:

> APPROVED under exception 5: wave llm-1 as proposed (opening store commit, ≤3 unit commits,
> merge into the wave branch, closing store commit, push through the b10x-gates bot, PR, merge to
> main when the full gate is green).
> (a) YES, after the merge to main: move each of the 4 stories to rejected only where llm covers
> its subject (cite the llm parity row or test). A story whose subject llm does not cover stays
> proposed, and you send me its one-line gap for llm.
> (b) YES: release 0.14.0 after the merge, through this repository's release process (bot tag,
> GitHub release; no crates.io). metaharness pins it for its b10x adapter (ADR 0097).
> platform/runtime/inference (repo `platform`, not session inference's repo `inference`) locks
> harness at af1da7ae (Cargo.lock), so only its next `cargo update` breaks; I record that for the
> platform owner, and you need not wait on it.

## What it authorises

The commits the wave page § *The commits approval authorises* lists, and in addition: the
conditional `proposed -> rejected` moves of the four superseded stories after the merge, each
citing the llm parity row or test that covers it; and release 0.14.0 after the merge through
`AGENTS.md` § Releases. Nothing else.
