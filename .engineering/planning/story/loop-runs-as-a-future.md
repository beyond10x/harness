---
format: aep.planning-md/3
id: story:loop-runs-as-a-future
kind: story
status: draft
title: The loop is a future, and run and run_in are its blocking facade
relations:
- decomposes: epic:async-native-loop
- depends_on: story:async-model-and-tool-ports
scope:
- confidence: cited
  path: crates/harness-loop/src/approval.rs
- confidence: cited
  path: crates/harness-loop/src/environment.rs
- confidence: cited
  path: crates/harness-loop/src/event.rs
- confidence: cited
  path: crates/harness-loop/src/hook.rs
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/tests.rs
revision: 7
---
## Outcome

The loop is written once, as a future: `AgentLoop::run_async` and `run_in_async` over the
asynchronous ports. Today's `run` and `run_in` keep their signatures and become a blocking facade
over that future, so every existing caller and test still compiles and passes (expand step).

## Evidence

- `crates/harness-loop/src/lib.rs:2148` (`run`), `:2189` (`run_in`) block the caller. Synchronous
  callers: `crates/harness-cli/src/lib.rs:2785`, `:2865`, `crates/harness-conformance/src/boundary.rs:115`,
  `crates/harness-app-server/src/lib.rs:589`, and about 131 call sites in
  `crates/harness-loop/src/tests.rs` (design critic, round 1).
- `run_in` writes the conversation and the spend back "on **every** exit path"
  (`lib.rs:2158-2188`). A dropped future runs no code after its last `.await`.
- Loop ports are synchronous: `ApprovalPort` (`approval.rs:42`), `TurnEnvironmentProvider`
  (`environment.rs:47`), `HookPort` (`hook.rs:139`), `LoopSink` (`event.rs:399`).
- Side-by-side delegates run on `std::thread::scope` (`lib.rs:3918`).
- `llm-blocking` refuses on any thread where `Handle::try_current()` is `Ok`
  (`llm/crates/llm-blocking/src/lib.rs:132`); a probe on tokio 1.54.1 (2026-10-29) printed
  `spawn_blocking=true`, so a `spawn_blocking` thread is refused. `uilab` runs Harness from
  `spawn_blocking` (`uilab/crates/uilab-app/src/app.rs:974`).

## Acceptance

- `run_async` and `run_in_async` take the asynchronous model and tool ports and asynchronous
  counterparts of the four loop ports, and return futures that are `Send` when those ports are.
- `run_in_async` writes each completed turn's items and spend into the caller's `items` and `spend`
  before awaiting the next request. A test drops the future after the first tool call and finds
  the first turn in `items` and its usage in `spend`.
- Side-by-side delegates run concurrently with no `std::thread::scope` in `lib.rs`
  (`grep -n 'thread::scope' crates/harness-loop/src/lib.rs` returns nothing); the delegate tests in
  `tests.rs` pass.
- `run` and `run_in` keep their signatures, wrap the synchronous ports with the adapters from
  `story:async-model-and-tool-ports`, and block on `run_in_async`. The unchanged `tests.rs` passes.
- `BlockingLoop::on_runtime` refuses a current-thread runtime handle with a named error, as
  `BlockingModel::on_runtime` does (`llm-blocking/src/lib.rs:87-98`); a test shows the refusal.
- Three tests run one fixture script through `run`: on a plain `std::thread` it answers; on a
  `tokio::task::spawn_blocking` thread it answers; inside an `async` task on a runtime worker it
  returns a named error and the fixture model records no request.
- A test awaits `run_async` from a `tokio::spawn`ed task inside `#[tokio::main]` with a fixture
  model, one tool call and one approval; the outcome equals the outcome `run` gives for that script.

## Scope

Recorded as `scope` entries. cited: `crates/harness-loop/src/lib.rs`, `approval.rs`,
`environment.rs`, `hook.rs`, `event.rs`, `tests.rs`. No dependency changes, so no manifest or lockfile.

## Out of Scope

Callers outside `harness-loop`; they move in `story:binary-and-catalogue-on-one-runtime`.
