---
format: aep.planning-md/3
id: approval-record:wave-substrate-adopt
kind: approval-record
status: draft
title: Wave substrate-adopt approved by the orchestrator under the class-C grant (D142)
tags:
- d142
relations:
- decides: story:daemon-exec-start-adoption
- decides: story:substrate-pin-0-12
- decides: dependency-blocker:substrate-daemon-admits-operator-named-workspaces
revision: 1
---
## The stop

Stage-1 proposal of wave substrate-adopt (`docs/plans/2026-10-28-wave-substrate-adopt.md`).

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-28, under the standing grant in the workspace
instructions (class C: a wave; dependency upgrades; commits, PRs and merges through the bot
route), decision D142. A relayed approval, not a bypass. Verbatim item:

"1. dependency-blocker substrate-daemon-admits-operator-named-workspaces: the substrate session
reports it delivered as `workspace.adopt` (substrate ADR 0035). Confirm in the substrate release
that ships it, clear the blocker with that evidence, and implement the harness side (harness calls
adopt) as a wave unit: failing test first, adversary after green.
2. story substrate-pin-0-12 is reported done but still active: verify and move it to implemented
with evidence.
3. Its gate still mints a token because it believes substrate is private; substrate has been public
since 0.2.1 (verify with `gh api repos/beyond10x/substrate --jq .private`). Remove the token step
if nothing else needs it.
4. Move harness's Substrate pin to the newest released Substrate if it is behind, in the same wave.
Cap 4 units; stop on the same stop reason twice, a red base at open, or a gate failing twice on one
cause. No release unless harness's AGENTS.md requires one for a pin move; report instead."

## What it authorises

The commits the wave page lists: the opening store commit, one commit per unit with its
corrections, the merges into `wave/2026-10-28-substrate-adopt`, the closing store commit, one pull
request to `main` through `b10x-gates api` and its merge by the App once the required checks are
green. No tag and no release: `AGENTS.md` § Releases does not require one for a pin move.
