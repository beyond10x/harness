---
format: aep.planning-md/3
id: story:approval-checkpoint-declared-in-spec
kind: story
status: archived
title: The approval checkpoint is declared in spec/
summary: harness.approval-checkpoint/1 (harness-loop/src/lib.rs:223,2187) is persisted by harness and resumed by agent-platform; declare it as an ESS entity. Draft, after 0.15.0.
relations:
- serves: vision:b10x-owns-its-loop
revision: 2
transitions:
- {from: "draft", to: "archived", at: "2026-10-20T08:52:15Z", actor: "human:timo", revision: 2}
---
## Context

`harness.approval-checkpoint/1` (`crates/harness-loop/src/lib.rs:223,2187`) is persisted by harness
and resumed by agent-platform — a cross-component contract the specification does not describe.
A consumer that writes and reads it has no typed description to hold against.

Survey by `b10x-chef-orchestrator`, 2026-10-20.

## Outcome

`harness.approval-checkpoint/1` is declared as an entity in `spec/`, so agent-platform's persist and
resume are held to a typed contract and `ess specify validate` checks it.

## Acceptance

`spec/` declares the `harness.approval-checkpoint/1` entity whose fields match
`crates/harness-loop/src/lib.rs:223,2187` (cite each field's source line); `ess specify validate`
passes; the declaration cites the Rust type and the `/1` version as its source.

## Out of scope

- Any change to how the checkpoint is persisted or resumed.
- The ledger (its own story) and the `--json` `LoopEvent` record (waits on ADR 0096).

## Sequencing

Draft only, after 0.15.0. ESS-domain work.
