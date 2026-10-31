---
format: aep.planning-md/3
id: review-result:awaitable-design-round-2
kind: review-result
status: active
title: Design critic, round 2, awaitable loop
relations:
- reviews: epic:loop-awaitable-from-async-programs
- reviews: story:loop-task-awaits-a-run
- reviews: story:real-ports-run-under-an-awaited-loop
- reviews: story:embedding-guide-recommends-loop-task
revision: 1
---
needs-revision

Round 1: finding 1 (construct `McpTools` on a runtime thread) resolved by D1's builder on the task thread; finding 3 (token source) resolved by D4; finding 2 resolved in D1, partly in the acceptance (fourth finding below).

- `story:loop-task-awaits-a-run` — the agreement acceptance cannot hold beside D4: `run-cancelled-before-provider` sets `"cancelled": true` and expects a `cancelled` stop; D4's "every later command returns a named error" is not tied to a dropped-future flag, so a pre-cancelled token would give that error instead. — `.engineering/planning/story/loop-task-awaits-a-run.md:65`, `:78`; `.engineering/planning/epic/loop-awaitable-from-async-programs.md:64-67`; `spec/scenarios/run-cancelled-before-provider.yaml:39,44-46`; `crates/harness-conformance/src/boundary.rs:106-108`
- `story:loop-task-awaits-a-run` — no output or error type for the futures: `LoopError` (`crates/harness-loop/src/lib.rs:329-345`) is exhaustive with no cancelled or panicked variant; the body never says where a builder `Err` surfaces or whether `spawn` blocks the async caller.
- `epic:loop-awaitable-from-async-programs` D3 — a boxed `dyn LoopSink + Send` cannot be turned back into a `VecLoopSink` (`crates/harness-loop/src/event.rs:399-401`); no acceptance says events reach the caller in order.
- `story:real-ports-run-under-an-awaited-loop` — with D6 (no join) a panic in the real `McpTools` or `Embedded` drop happens on a detached thread and cannot fail the test; needs an observable within the 30 s bound.

Out of lane: the guide does not say dropping a future ends the task; stale "Send gaps" summary; whether a command is sent on call or on first poll.

```findings
- file: .engineering/planning/story/loop-task-awaits-a-run.md
  line: 65
  category: design
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "D4's 'every later command returns a named error' is not tied to a dropped-future flag, so a task whose token the builder pre-cancels would return that error instead of the cancelled stop that scenario run-cancelled-before-provider expects, and the 17-scenario agreement acceptance cannot hold; the body must pin the trigger and say how the cancelled scenario is driven"
- file: .engineering/planning/story/loop-task-awaits-a-run.md
  line: 73
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body names no output or error type for the futures: LoopError is exhaustive with no cancelled or panicked variant, so the named errors either add variants against D5 or need a new type, and the body never says where a builder Err surfaces or whether spawn blocks the async caller"
- file: .engineering/planning/epic/loop-awaitable-from-async-programs.md
  line: 59
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "D3 says the Box<dyn LoopSink + Send> sink 'comes back with the result', but LoopSink has only emit so a boxed sink cannot be turned back into a VecLoopSink, and no story acceptance says events reach the caller in order"
- file: .engineering/planning/story/real-ports-run-under-an-awaited-loop.md
  line: 61
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the only acceptance for where runtime-owning ports are dropped is 'drops the LoopTask and does not panic', but with D6 (no join) a panic in the real McpTools or Embedded drop happens on a detached thread and cannot fail the test; add an observable within the 30 s bound"
```
