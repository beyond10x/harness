---
format: aep.planning-md/3
id: story:run-ledger-declared-in-spec
kind: story
status: archived
title: The run ledger is declared in spec/ for ADR 0101 attribution
summary: RunLedger (harness-loop/src/lib.rs:273) is the harness spend record consumers read and ADR 0101 attributes from; declare it as an ESS entity. Draft, after 0.15.0.
relations:
- serves: vision:b10x-owns-its-loop
revision: 2
transitions:
- {from: "draft", to: "archived", at: "2026-10-20T08:52:14Z", actor: "human:timo", revision: 2}
---
## Context

`RunLedger` (`crates/harness-loop/src/lib.rs:273`) is the harness layer's spend record — what Atlas
ADR 0101 (trials measure every layer) attributes a run's cost from. Consumers read it: agentide
(`harness_tui.rs:17`) and uilab (`crates/uilab-agent/src/lib.rs:23`). It has no `serde` derive today,
and `spec/loop.yaml` declares only `Budget` and `Stop`, so the ledger is a type two consumers depend
on that the specification does not describe.

Survey by `b10x-chef-orchestrator`, 2026-10-20.

## Outcome

`RunLedger` is declared as an entity in `spec/` (its fields, their types, what each counts), so a
consumer and ADR 0101's attribution read the same typed description, and `ess specify validate`
checks it.

## Acceptance

`spec/` declares a `RunLedger` entity whose fields match `crates/harness-loop/src/lib.rs:273` (cite
each field's source line); `ess specify validate --path <spec>` passes; the declaration cites the
Rust type as its source. Whether `RunLedger` also gains a `serde` derive is decided in this story
and recorded (a consumer that serialises it needs one).

## Out of scope

- The `--json` `LoopEvent` record: left until Atlas ADR 0096's open point on trace formats is
  settled.
- Changing what the ledger counts.

## Sequencing

Draft only, after 0.15.0. ESS-domain work (ADR 0101).
