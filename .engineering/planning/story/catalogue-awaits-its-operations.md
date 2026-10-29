---
format: aep.planning-md/3
id: story:catalogue-awaits-its-operations
kind: story
status: archived
title: The catalogue's Operations are awaited, and synchronous tool calls leave the runtime worker
relations:
- decomposes: epic:async-native-loop
- depends_on: story:async-model-and-tool-ports
scope:
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: crates/harness-catalogue/Cargo.toml
- confidence: inferred
  path: crates/harness-catalogue/src/catalogue.rs
- confidence: cited
  path: crates/harness-catalogue/src/flat.rs
- confidence: cited
  path: crates/harness-catalogue/src/local.rs
- confidence: cited
  path: crates/harness-catalogue/src/operations.rs
- confidence: cited
  path: crates/harness-catalogue/src/tests.rs
- confidence: cited
  path: crates/harness-catalogue/src/verbs.rs
- confidence: cited
  path: crates/harness-substrate/src/tools.rs
revision: 4
transitions:
- {from: "draft", to: "archived", at: "2026-10-29T03:13:27Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"review_outcome":1}}}
---
## Outcome

The tool catalogue serves the asynchronous `ToolPort`, its `Operations` trait is asynchronous, and
every synchronous filesystem, search or process call runs on tokio's blocking pool, never on a
runtime worker (migrate step).

## Evidence

- `Operations` is a synchronous `Send + Sync` trait (`crates/harness-catalogue/src/operations.rs:196`).
  Implementors: `Split` (`operations.rs:394`), `LocalOperations` (`crates/harness-catalogue/src/local.rs:198`),
  `ConfinedOperations` (`crates/harness-substrate/src/tools.rs:153`), and two test fixtures
  (`crates/harness-catalogue/src/tests.rs:25`, `:1196`).
- `ToolPort` implementors: `crates/harness-catalogue/src/verbs.rs:269`, `:343`,
  `crates/harness-catalogue/src/flat.rs:130`, `:185`.
- The catalogue adapts the `tools` repository's toolsets, whose `Toolset::invoke` is synchronous
  (`tools/crates/tool-contract/src/conformance.rs:568`) and stays so (epic, Out of Scope).

## Acceptance

- `Operations`' methods return futures; `LocalOperations` runs each synchronous call through
  `tokio::task::spawn_blocking`.
- `Flat` and `Verbs` implement `AsyncToolPort`; their synchronous `ToolPort` implementations stay
  until `story:blocking-facade-keeps-sync-callers`.
- A `#[tokio::test(flavor = "current_thread")]` calls a catalogue tool whose `LocalOperations`
  read waits 50 ms while a second task increments a counter every millisecond; the counter has
  advanced when the call returns.
- The existing `harness-catalogue` tests pass.

## Scope

Recorded as `scope` entries. cited: `crates/harness-catalogue/src/operations.rs`,
`crates/harness-catalogue/src/local.rs`, `crates/harness-catalogue/src/verbs.rs`,
`crates/harness-catalogue/src/flat.rs`, `crates/harness-catalogue/src/tests.rs`,
`crates/harness-substrate/src/tools.rs`. inferred: `crates/harness-catalogue/src/catalogue.rs`,
`crates/harness-catalogue/Cargo.toml`, `Cargo.lock`.

## Depends on

`story:async-model-and-tool-ports`.
