---
format: aep.planning-md/3
id: story:loop-task-awaits-a-run
kind: story
status: implemented
title: LoopTask awaits a run, an approval resume and a single call
summary: Owned ports, a dedicated run thread with no runtime context, a waker-completed future, cancel on drop.
relations:
- decomposes: epic:loop-awaitable-from-async-programs
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: crates/harness-conformance/src/boundary.rs
- confidence: cited
  path: crates/harness-loop/Cargo.toml
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/task.rs
- confidence: cited
  path: crates/harness-loop/tests/adversary_loop_task.rs
- confidence: cited
  path: crates/harness-loop/tests/adversary_loop_task_pass2.rs
- confidence: cited
  path: crates/harness-loop/tests/awaited.rs
revision: 12
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T03:35:23Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"review_outcome":7}}}
- {from: "proposed", to: "active", at: "2026-10-29T03:35:23Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"review_outcome":7}}}
- {from: "active", to: "implemented", at: "2026-10-29T04:51:26Z", actor: "human:timo", revision: 12, decided_on: {"recorded":{"test_result":1,"review_outcome":9,"verification":1}}}
---
## Outcome

`harness_loop::LoopTask` builds a run's ports on a thread it starts and keeps them there. Its
`run`, `run_in`, `resume_approval` and `call` return runtime-neutral futures that execute the
existing synchronous loop on that thread. Dropping an unfinished future cancels the task's token;
dropping the task lets the thread drop the ports and exit (epic D1–D6).

## Evidence

- Entry points to mirror: `AgentLoop::call` (`crates/harness-loop/src/lib.rs:2120`), `run`
  (`:2148`), `run_in` (`:2189`), `resume_approval` (`:2227`); builders `with_cancel` (`:2081`),
  `with_hooks` (`:2088`), `with_environment` (`:2098`).
- Cancellation is `harness_wire::Cancel`, with `new`, `cancel` and `is_cancelled` and no reset
  (`crates/harness-wire/src/cancel.rs:15-24`).
- The conformance boundary drives `harness.loop.Run` through `run_in` and `harness.loop.Call`
  through `call` (`crates/harness-conformance/src/boundary.rs:70-82`,
  `spec/domains/loop.yaml:48,67`); `spec/scenarios/` holds 17 `run-*` and `call-*` scenarios.

## Acceptance

Named tests in `crates/harness-loop` unless stated otherwise. Every test that waits on the run
thread bounds the wait at 5 s and fails on timeout.

- `awaited_run_leaves_the_runtime_thread_free`: on a `current_thread` tokio runtime, a run whose
  model turn blocks until a spawned task has incremented a shared counter once is awaited; the run
  completes. (Run inline on that runtime, the spawned task never runs and the test times out.)
- `awaited_run_has_no_runtime_context`: inside the model port's `turn`,
  `tokio::runtime::Handle::try_current()` is `Err`.
- `ports_are_built_on_the_task_thread`: the builder records `std::thread::current().id()`; it
  differs from the test's thread and equals the id the model port records in `turn`.
- `awaited_run_in_returns_items_and_spend`: `run_in` returns the conversation and the `RunLedger`
  on success and on a wire error, as the synchronous `run_in` writes them back.
- `dropping_the_awaited_run_cancels_the_ports_token`: a model port built with the token the
  builder receives blocks in `turn` until that token is cancelled; the run future is dropped; the
  port observes the cancel; a following `run` on the same task resolves to `Ok` with stop
  `cancelled` and the model port records no further turn.
- `dropping_the_task_drops_the_ports_on_its_thread`: a port whose `Drop` records its thread id;
  after the `LoopTask` is dropped inside an async test, the recorded id is the task thread's.
- `shutdown_waits_for_the_ports_to_drop` and `shutdown_reports_a_panicking_drop`: `shutdown()`
  resolves after a port's `Drop` has run; a port whose `Drop` panics makes it resolve to the panic
  error.
- `spawn_does_not_wait_for_the_builder` and `a_failing_builder_resolves_the_first_future`: `spawn`
  returns while the builder is still blocked; a builder that returns `Err` makes the first `run`
  resolve to the builder error.
- `events_reach_the_sink_in_order`: the `VecLoopSink` returned by `run` holds the same events in
  the same order as the synchronous `run` writes for the same scripted ports.
- `commands_run_in_the_order_sent`: two `call`s issued before either is awaited reach the tool
  port in issue order.
- `awaited_approval_resumes`: a run that stops awaiting approval returns its checkpoint;
  `resume_approval` on the same task completes the run.
- `awaited_call_meets_the_approver`: `call` with a denying approver returns the refusal outcome
  the synchronous `call` returns.
- `a_panicking_port_resolves_the_future`: a port that panics resolves the future to an error that
  names the panic.
- `the_future_needs_no_tokio`: the run is awaited with a minimal executor written in the test
  (thread park and unpark), no tokio.
- `cargo tree -p b10x-harness-loop -e normal` lists no tokio.
- A unit test in `crates/harness-conformance/src/` (the crate has no library target),
  `loop_task_and_agent_loop_agree_on_every_run_and_call_scenario`, executes each `run-*` and
  `call-*` scenario in `spec/scenarios/` through both `AgentLoop` and `LoopTask`, asserts the two
  JSON responses are equal, and asserts it compared 17 scenarios. A scenario with
  `cancelled: true` is driven through `cancel_handle().cancel()` before the run.
- `CHANGELOG.md` `[Unreleased]` lists `LoopTask` and `LoopTaskError` under Added.
- Tests that use `flavor = "multi_thread"` declare tokio's `rt-multi-thread` feature in the
  crate's `[dev-dependencies]` (the workspace declares `rt`, `time`, `macros`, `Cargo.toml:59`).
- `cargo test -p b10x-harness-loop -p b10x-harness-conformance` and `cargo xtask specification`
  pass.

## Scope

Recorded as `scope` entries.

## Depends on

Nothing.
