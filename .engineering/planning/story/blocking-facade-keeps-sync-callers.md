---
format: aep.planning-md/3
id: story:blocking-facade-keeps-sync-callers
kind: story
status: draft
title: The asynchronous ports take the plain names, the synchronous duplicates go, and 0.23.0 says so
relations:
- decomposes: epic:async-native-loop
- depends_on: story:binary-and-catalogue-on-one-runtime
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: Cargo.toml
- confidence: inferred
  path: README.md
- confidence: inferred
  path: crates/harness-app-server/src
- confidence: inferred
  path: crates/harness-catalogue/src
- confidence: inferred
  path: crates/harness-cli/src
- confidence: inferred
  path: crates/harness-conformance/src
- confidence: cited
  path: crates/harness-llm/Cargo.toml
- confidence: cited
  path: crates/harness-llm/src/lib.rs
- confidence: inferred
  path: crates/harness-loop/src/adapt.rs
- confidence: inferred
  path: crates/harness-loop/src/approval.rs
- confidence: inferred
  path: crates/harness-loop/src/environment.rs
- confidence: inferred
  path: crates/harness-loop/src/event.rs
- confidence: inferred
  path: crates/harness-loop/src/hook.rs
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-mcp/src/lib.rs
- confidence: cited
  path: crates/harness-substrate/src/embedded.rs
- confidence: cited
  path: crates/harness-wire/src/port.rs
revision: 5
---
## Outcome

The asynchronous ports take the plain names, the synchronous ones are named for what they are, the
duplicate synchronous implementations are gone, and the release says the port change is breaking
(contract step). A synchronous consumer keeps working through `run`, `run_in` and the `Blocking`
adapters.

## Evidence

- After the expand and migrate stories, each port exists twice: `ModelPort` and `AsyncModelPort`,
  `ToolPort` and `AsyncToolPort`, `StreamSink` and `AsyncStreamSink`, and the four loop ports and
  their asynchronous counterparts (`story:async-model-and-tool-ports`, `story:loop-runs-as-a-future`).
- `crates/harness-llm/src/lib.rs:370` uses `BlockingModel::new`; `:494` and `:1441` build
  current-thread runtimes; `crates/harness-mcp/src/lib.rs:112` and
  `crates/harness-substrate/src/embedded.rs:163` build runtimes. The migrate stories add the
  asynchronous path beside each; this story deletes the synchronous one.
- Workspace version `0.22.0` (`Cargo.toml:18`). Port implementors outside this repository:
  `uilab` 8 files, `agentide` 2, `agent-platform` 2, `control-plane` 1 (grep, 2026-10-29).

## Acceptance

- `ModelPort`, `ToolPort`, `StreamSink`, `ApprovalPort`, `HookPort`, `TurnEnvironmentProvider` and
  `LoopSink` are the asynchronous traits; the synchronous ones are `BlockingModelPort`,
  `BlockingToolPort`, `BlockingStreamSink`, `BlockingApprovalPort`, `BlockingHookPort`,
  `BlockingTurnEnvironmentProvider` and `BlockingLoopSink`. `grep -rn 'AsyncModelPort\|AsyncToolPort' crates`
  returns nothing.
- `grep -rn 'new_current_thread\|new_multi_thread\|block_on\|BlockingModel' crates/harness-llm/src crates/harness-mcp/src crates/harness-substrate/src`
  returns nothing, and `crates/harness-llm/Cargo.toml` names no `b10x-llm-blocking`.
- `CHANGELOG.md` has an entry for the release under a `Breaking` heading naming the port rename and
  `run_async`, and the workspace version is `0.23.0`.
- `cargo test --workspace --locked` and `cargo xtask specification` pass.

## Scope

Recorded as `scope` entries. cited: `crates/harness-wire/src/port.rs`, `crates/harness-loop/src/lib.rs`,
`crates/harness-llm/src/lib.rs`, `crates/harness-llm/Cargo.toml`, `crates/harness-mcp/src/lib.rs`,
`crates/harness-substrate/src/embedded.rs`, `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`. inferred:
`crates/harness-loop/src/adapt.rs`, `approval.rs`, `environment.rs`, `hook.rs`, `event.rs`,
`crates/harness-catalogue/src`, `crates/harness-cli/src`, `crates/harness-app-server/src`,
`crates/harness-conformance/src`, `README.md`.

## Out of Scope

Consumers in other repositories, and `llm-blocking`'s runtime guard.
