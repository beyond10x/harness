---
format: aep.planning-md/3
id: story:binary-and-catalogue-on-one-runtime
kind: story
status: draft
title: Binary, app server and conformance target await the loop on one runtime per process
relations:
- decomposes: epic:async-native-loop
- depends_on: story:loop-runs-as-a-future
- depends_on: story:harness-llm-awaits-the-model
- depends_on: story:mcp-and-substrate-await-their-drivers
- depends_on: story:catalogue-awaits-its-operations
scope:
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: crates/harness-app-server/Cargo.toml
- confidence: cited
  path: crates/harness-app-server/src/lib.rs
- confidence: cited
  path: crates/harness-app-server/src/session.rs
- confidence: inferred
  path: crates/harness-catalogue/tests
- confidence: inferred
  path: crates/harness-cli/Cargo.toml
- confidence: cited
  path: crates/harness-cli/src/approve.rs
- confidence: cited
  path: crates/harness-cli/src/caller.rs
- confidence: cited
  path: crates/harness-cli/src/hooks.rs
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-cli/src/main.rs
- confidence: inferred
  path: crates/harness-cli/tests
- confidence: cited
  path: crates/harness-conformance/src/boundary.rs
- confidence: inferred
  path: crates/harness-xtask/src/main.rs
revision: 5
---
## Outcome

The binary, the app server and the conformance target await the loop, each process builds one
runtime, and the binary's argv contract is unchanged (migrate step).

## Evidence

- `crates/harness-cli/src/main.rs:3` is a synchronous `fn main`; the loop is called at
  `crates/harness-cli/src/lib.rs:2785`, `:2865`.
- Loop-port implementors in the binary: `crates/harness-cli/src/approve.rs:25`, `caller.rs:53`,
  `hooks.rs:58`; tool-port implementors `crates/harness-cli/src/lib.rs:3159`, `:3265`
  (parallel-safety critic, round 1).
- App server: loop call at `crates/harness-app-server/src/lib.rs:589`; `impl ToolPort for BridgeTools`
  at `crates/harness-app-server/src/session.rs:499`.
- Conformance: `crates/harness-conformance/src/boundary.rs:20` (model), `:60` (tools), `:115` (run).
- `cargo xtask specification` runs the ESS suite against the real libraries (AGENTS.md
  § Executable specification). The argv surface is pinned under `contracts/cli/`.

## Acceptance

- `harness-cli` builds exactly one runtime: `grep -rn 'runtime::Builder' crates/harness-cli/src`
  prints one line, in `main.rs`; `lib.rs` awaits `run_in_async`.
- `grep -rn 'runtime::Builder\|block_on' crates/harness-app-server/src` returns nothing; the app
  server awaits `run_in_async` on the runtime it is served on.
- `approve.rs`, `caller.rs`, `hooks.rs`, `BridgeTools` and the conformance ports implement the
  asynchronous traits.
- `cargo xtask specification` passes with `boundary.rs` driving `run_in_async`.
- The gate's CLI contract drift check passes with `git diff --stat contracts/cli` empty.
- The `harness-cli`, `harness-app-server` and `harness-catalogue` test suites pass.

## Scope

Recorded as `scope` entries. cited: `crates/harness-cli/src/main.rs`, `crates/harness-cli/src/lib.rs`,
`crates/harness-cli/src/approve.rs`, `crates/harness-cli/src/caller.rs`, `crates/harness-cli/src/hooks.rs`,
`crates/harness-app-server/src/lib.rs`, `crates/harness-app-server/src/session.rs`,
`crates/harness-conformance/src/boundary.rs`. inferred: `crates/harness-cli/tests`,
`crates/harness-catalogue/tests`, `crates/harness-xtask/src/main.rs`, `crates/harness-cli/Cargo.toml`,
`crates/harness-app-server/Cargo.toml`, `Cargo.lock`.

## Depends on

`story:loop-runs-as-a-future`, `story:harness-llm-awaits-the-model`,
`story:catalogue-awaits-its-operations`, `story:mcp-and-substrate-await-their-drivers`.
