---
format: aep.planning-md/3
id: story:harness-loop-modeled-in-ess
kind: story
status: active
title: 'Harness''s loop is modeled in ESS: ledger, approval checkpoint, turn/tool/compaction states and conformance'
summary: One ESS modeling story folding the run ledger and approval-checkpoint drafts plus the loop's turn, tool round trip and compaction states, with a conformance suite against the real loop. Draft, after 0.15.0; ess:specifying.
relations:
- serves: vision:b10x-owns-its-loop
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-10-30T17:39:19Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-30T17:39:19Z", actor: "human:timo", revision: 3}
---
## Context

Harness's inner workings are not modeled in ESS. Several are cross-component contracts a consumer or
a trial already depends on, with no typed description to hold against:

- **`RunLedger`** (`crates/harness-loop/src/lib.rs:273`) — the harness layer's spend record Atlas
  ADR 0101 attributes a run's cost from; read by agentide (`harness_tui.rs:17`) and uilab
  (`crates/uilab-agent/src/lib.rs:23`). No `serde` today; `spec/loop.yaml` declares only `Budget`
  and `Stop`.
- **`harness.approval-checkpoint/1`** (`crates/harness-loop/src/lib.rs:223,2187`) — persisted by
  harness and resumed by agent-platform.
- **The loop's own states** — a turn, a tool round trip, and compaction — which the specification
  does not describe as entities/transitions.

Requested by Timo (relayed by `b10x-chef-orchestrator`, 2026-10-20 ~10:55), folding the two
~10:50 ESS drafts (`story:run-ledger-declared-in-spec`,
`story:approval-checkpoint-declared-in-spec`, now archived into this one).

## Outcome

`spec/` describes harness's loop: `RunLedger`, `harness.approval-checkpoint/1`, and the turn /
tool-round-trip / compaction states and their transitions, with a conformance suite that holds the
real `harness-loop` to them. A consumer and ADR 0101's attribution read the same typed description.

## Acceptance

`spec/` declares, with each field and state citing its `crates/harness-loop/src/lib.rs` source line:
`RunLedger`; `harness.approval-checkpoint/1` (version `/1`); and the loop states (turn, tool round
trip, compaction) with their legal transitions. `ess specify validate` passes, and an
`ess-conformance` suite executes against the real loop (not a fixture) and is green. Built with the
`ess:specifying` skill (and `ess:testing-conformance` for the suite). Whether `RunLedger` gains a
`serde` derive is decided here and recorded.

## Out of scope

- The `--json` `LoopEvent` record: left until Atlas ADR 0096's open point on trace formats is
  settled.
- Any change to loop behaviour, the ledger's contents, or how the checkpoint is persisted.

## Sequencing

Draft only, after 0.15.0. ESS-domain work (ADR 0101).
