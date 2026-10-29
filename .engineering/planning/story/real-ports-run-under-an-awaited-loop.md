---
format: aep.planning-md/3
id: story:real-ports-run-under-an-awaited-loop
kind: story
status: active
title: The model, MCP and embedded Substrate ports run under LoopTask on both tokio flavours
summary: End-to-end tests from tokio, both flavours, for the shipped ports built and dropped inside the LoopTask builder's thread.
relations:
- decomposes: epic:loop-awaitable-from-async-programs
- depends_on: story:loop-task-awaits-a-run
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: cited
  path: crates/harness-cli/Cargo.toml
- confidence: cited
  path: crates/harness-cli/tests/awaited_ports.rs
- confidence: inferred
  path: crates/harness-llm/src/lib.rs
- confidence: inferred
  path: crates/harness-mcp/src/lib.rs
- confidence: inferred
  path: crates/harness-substrate/src/embedded.rs
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T03:35:23Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"review_outcome":7}}}
- {from: "proposed", to: "active", at: "2026-10-29T03:35:24Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"review_outcome":7}}}
---
## Outcome

The shipped ports (the `harness_llm` model, `harness_mcp::McpTools`, and the catalogue over the
embedded Substrate backend) run under `LoopTask` from a tokio program on both runtime flavours.
They are built in the `LoopTask` builder, so on the task's thread, and dropped there.

## Evidence

- `LlmPort::turn` escapes to a scoped thread (`crates/harness-llm/src/lib.rs:558-565`) and checks
  the token it was built with (`:529`, `:546`).
- `McpTools::prepare` calls `Runtime::block_on` (`crates/harness-mcp/src/lib.rs:117,129`) and
  `McpTools::call_within` does too (`:202`); the embedded backend owns a `Runtime`
  (`crates/harness-substrate/src/embedded.rs:82,163`) and blocks on it (`:315-492`). Built or
  dropped on a runtime thread, these panic (`review-result:awaitable-design-round-1`).
- The stdio MCP fixture is `controlled_fixture_binary()` with `args = ["stdio"]`
  (`crates/harness-cli/tests/end_to_end.rs:324-343`).
- File operations over the embedded backend need no cgroup (`crates/harness-substrate/tests/embedded_live.rs:15-30`).
- That these paths work from an async program is inferred from the code; no test runs them there
  today.

## Acceptance

Named tests in `crates/harness-cli/tests/awaited_ports.rs` (the stdio fixture helper
`controlled_fixture_binary` is private to `end_to_end.rs:104`, so the new file carries its own
copy or both share a `tests/common` module), each in a `_current_thread` and a
`_multi_thread` variant (`#[tokio::test(flavor = …)]`), each bounding its wait at 30 s:

- `llm_port_runs_under_loop_task_{current_thread,multi_thread}`: an `LlmPort` built in the builder
  against a local fixture endpoint answers one turn through `LoopTask::run`; the outcome's text is
  the fixture's.
- `mcp_tools_run_under_loop_task_{current_thread,multi_thread}`: `McpTools` built in the builder
  over the stdio fixture; a scripted model calls one fixture tool; the tool's result is in the
  returned items.
- `embedded_file_operation_runs_under_loop_task_{current_thread,multi_thread}`: the catalogue over
  the embedded backend, built in the builder; a scripted model calls one file operation; its result
  is in the returned items. No skip path: the test runs on every host the existing
  `embedded_live.rs` file-operation tests run on.
- Each test ends with `task.shutdown().await` inside the async body, within the 30 s bound, and
  asserts it is `Ok`: the real `McpTools` or `Embedded` runtime was dropped on the task thread
  without a panic.
- `crates/harness-cli/Cargo.toml` `[dev-dependencies]` enables tokio `rt-multi-thread` and
  `macros`.
- A production change in `harness-llm`, `harness-mcp` or `harness-substrate`, if one is needed,
  carries a `CHANGELOG.md` `[Unreleased]` line.
- `cargo test -p b10x-harness-cli --test awaited_ports` passes.

## Scope

Recorded as `scope` entries.

## Depends on

`story:loop-task-awaits-a-run`.
