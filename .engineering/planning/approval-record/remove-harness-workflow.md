---
format: aep.planning-md/3
id: approval-record:remove-harness-workflow
kind: approval-record
status: draft
title: Timo approved removing the workflow verb and harness events in 0.18.0, breaking metaharness-aep-eval native
tags:
- b10x-chef-orchestrator
relations:
- decides: story:workflow-verb-is-removed
- decides: story:harness-events-is-retired
revision: 1
---
## The stop

Removal of the `workflow` verb (`workflow plan`, `workflow run`), the `harness-flow` crate if
nothing else uses it, design 0003's runner and the README § Workflows: a deliberate change to the
command-line contract, cut as a new dated `contracts/cli/b10x-harness/` version with a CHANGELOG
entry under Removed, released as 0.18.0.

## Who decided

Timo, 2026-10-22 ~04:4x, relayed by session `b10x-chef-orchestrator` under its grant. Verbatim:
"remove harness workflow". The orchestrator added the grounds: Atlas ADR 0096 puts multi-step
governed work in Loom (L3), and a `git grep` on main found no caller in metaharness, takt or loom
(one comment in `loom/crates/loom-executor/tests/adversary_ess_gate.rs:282`).

## What it authorises

The removal, the new contract cut, and release 0.18.0, together with the `harness events` removal
if that is ready.

## Open on 2026-10-22

The grounds do not hold for metaharness: its `origin/main` (`a34551e`) runs `b10x-harness workflow
plan` and `workflow run` from `crates/metaharness-aep-eval/src/lib.rs:260`, `:297`, `:907` and `:996`
(`aep-eval native`, metaharness `AGENTS.md:172`). Removing the verb breaks that consumer, which
the orchestrator's grant does not cover. Asked the same day, the orchestrator chose option A:
"A. Ship 0.18.0 with the `harness events` removal and the docs work. The workflow removal goes in
the first release after metaharness's aep-eval stops calling it; I've asked metaharness for that
and you'll hear from me." It confirmed the callers on metaharness `origin/main`: `workflow` appears
only in `crates/metaharness-aep-eval/src/lib.rs` (260, 297, 907, 996, 1185, 1938).

Superseded the same morning. Timo, ~05:0x, relayed by the orchestrator as option B: "Yeah, no, I
want you to ship this." Timo approves breaking `aep-eval`'s native arm against 0.18.0; the
orchestrator told metaharness to drop the dependency in parallel. So 0.18.0 removes `workflow`
together with `harness events`, and the CHANGELOG's Removed entry names `metaharness-aep-eval` as
the consumer that breaks.
