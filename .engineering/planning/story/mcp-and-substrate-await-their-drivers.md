---
format: aep.planning-md/3
id: story:mcp-and-substrate-await-their-drivers
kind: story
status: draft
title: The MCP tool port and the embedded Substrate backend own no runtime
relations:
- decomposes: epic:async-native-loop
- depends_on: story:async-model-and-tool-ports
- depends_on: story:catalogue-awaits-its-operations
scope:
- confidence: cited
  path: crates/harness-mcp/src/lib.rs
- confidence: cited
  path: crates/harness-substrate/src/embedded.rs
- confidence: cited
  path: crates/harness-substrate/src/tools.rs
- confidence: cited
  path: crates/harness-substrate/tests/conformance.rs
revision: 6
---
## Outcome

The MCP tool port and the embedded Substrate backend await their drivers on the caller's runtime,
and a batch of reads on the embedded backend runs concurrently (migrate step).

## Evidence

- `crates/harness-mcp/src/lib.rs:85` holds a `tokio::runtime::Runtime`; `:112` builds it
  current-thread; `:117`, `:129`, `:202` `block_on` registry load, connect and call.
- `crates/harness-substrate/src/embedded.rs:82` holds a runtime; `:163` builds it current-thread;
  `:315`, `:344`, `:390`, `:462`, `:480`, `:491` `block_on` each operation. Its module docs say a
  current-thread runtime serialises a batch (`embedded.rs:32-41`).
- `impl ToolPort for McpTools` at `harness-mcp/src/lib.rs:168`; `impl Operations for ConfinedOperations`
  at `crates/harness-substrate/src/tools.rs:153`.

## Acceptance

- `McpTools` implements `AsyncToolPort` and awaits registry load, connect and call; the
  `harness-mcp` tests pass.
- `ConfinedOperations` implements the asynchronous `Operations` from
  `story:catalogue-awaits-its-operations` by awaiting the embedded backend's operations.
- A test issues two reads in one batch against an embedded backend whose read handler waits on a
  `tokio::sync::Barrier` of 2; the batch completes, which a serial backend cannot do.
- `embedded.rs`'s module docs state that a batch runs concurrently and no longer describe a
  private runtime.
- `crates/harness-substrate/tests/conformance.rs` passes.

## Scope

Recorded as `scope` entries. cited: `crates/harness-mcp/src/lib.rs`,
`crates/harness-substrate/src/embedded.rs`, `crates/harness-substrate/src/tools.rs`,
`crates/harness-substrate/tests/conformance.rs`. Both crates already depend on tokio, so no manifest or lockfile.

## Depends on

`story:async-model-and-tool-ports`, `story:catalogue-awaits-its-operations`.
