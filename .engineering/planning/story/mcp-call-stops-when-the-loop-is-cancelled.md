---
format: aep.planning-md/3
id: story:mcp-call-stops-when-the-loop-is-cancelled
kind: story
status: draft
title: An MCP tool call stops when the loop is cancelled
summary: McpTools takes the loop's cancel token, so a dropped LoopTask run or Ctrl-C ends an MCP call at once instead of after the 30 s request timeout.
relations:
- decomposes: epic:async-native-loop
scope:
- confidence: inferred
  path: crates/harness-cli/tests/adversary_awaited_ports.rs
- confidence: inferred
  path: crates/harness-cli/tests/awaited_ports.rs
- confidence: inferred
  path: crates/harness-mcp/src/lib.rs
revision: 3
---
## Outcome

A cancelled loop stops waiting on an MCP tool call at once: when the `LoopTask` token is
cancelled (a dropped run future, `cancel_handle().cancel()`, or Ctrl-C in the CLI), the call in
flight ends within a bound far below the MCP request timeout, and `shutdown()` resolves promptly.

## Evidence

- Found by `review-result:real-ports-adversary-pass-1` (finding 1, pre-existing): `McpTools`
  never sees the task's `LoopCancel`. `McpTools::prepare(profile_path, registry_path)` takes no
  token (`crates/harness-mcp/src/lib.rs:99`), and `call_within` blocks on
  `connection.call(&request, remaining)` (`:188-202`) until the MCP client's default request
  timeout of 30 s (`b10x-mcp-types/src/lib.rs:75`).
- Measured: with a stdio endpoint that never answers `tools/call`, a run future dropped mid-call
  leaves `shutdown()` pending for 30.16 s (adversary run, bound raised to 60 s).
- `crates/harness-cli/tests/adversary_awaited_ports.rs`
  (`adversary_mcp_shutdown_waits_for_the_request_timeout_after_a_dropped_call_*`) asserts today's 30 s wait and names
  this story; flip it to the prompt bound when this lands.
- The synchronous CLI's Ctrl-C has the same gap (inferred from the same code path; not run).

## Acceptance

- `McpTools` can be built with the loop's cancel token (an additive constructor; `prepare` keeps
  working), and the `LoopTask` builder in `awaited_ports.rs` passes it.
- `adversary_mcp_shutdown_waits_for_the_request_timeout_after_a_dropped_call_*` are rewritten to assert
  `shutdown()` resolves `Ok` within 5 s of the run's future being dropped mid-call.
- A synchronous `AgentLoop` run whose token is cancelled mid-MCP-call returns `Ok` with stop
  `cancelled` within 5 s.
- Whether the MCP client (`beyond10x/mcp`) must expose a cancellable call is settled first: if
  `connection.call` cannot be interrupted, the change starts there and this story depends on it.

## Scope

Recorded as `scope` entries once scoped.
