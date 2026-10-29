---
format: aep.planning-md/3
id: review-result:awaitable-acceptance-round-2
kind: review-result
status: active
title: Acceptance critic, round 2, awaitable loop
relations:
- reviews: epic:loop-awaitable-from-async-programs
- reviews: story:loop-task-awaits-a-run
- reviews: story:real-ports-run-under-an-awaited-loop
- reviews: story:embedding-guide-recommends-loop-task
revision: 1
---
needs-revision

Round 1: all eight findings resolved (package name, 17-scenario comparison with count, builder on the task thread, Send record removed, no skip path, compiled examples, real `harness-docs generate --check`, interim-thread sentence).

- story:loop-task-awaits-a-run — "a following `run` on the same task resolves to the named cancelled error" names no error, while the synchronous loop returns a pre-cancelled token as `Ok` with stop `cancelled`, which the agreement item must show equal for `run-cancelled-before-provider` — `.engineering/planning/story/loop-task-awaits-a-run.md:46`, `spec/scenarios/run-cancelled-before-provider.yaml:55-58`, `crates/harness-loop/src/lib.rs:2672-2673`
- story:real-ports-run-under-an-awaited-loop — `cargo test -p b10x-harness-cli --test <the new file>` is a placeholder — `.engineering/planning/story/real-ports-run-under-an-awaited-loop.md:67`
- story:embedding-guide-recommends-loop-task — the rustdoc bullet gives a command only for the doctest; the links from `AgentLoop`, `run`, `run_in`, `resume_approval` and `call` are unchecked — `.engineering/planning/story/embedding-guide-recommends-loop-task.md:50`

Out of lane: `crates/harness-conformance` is binary-only (no `lib.rs`); `flavor = "multi_thread"` needs `rt-multi-thread` (`Cargo.toml:59` has `rt, time, macros`); `controlled_fixture_binary` is private to `end_to_end.rs:104`.

```findings
- file: .engineering/planning/story/loop-task-awaits-a-run.md
  line: 46
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "a following `run` on the same task resolves to the named cancelled error, but no item names the error, and the synchronous loop returns a pre-cancelled token as Ok with stop cancelled, which the agreement item must show equal for run-cancelled-before-provider; the story does not say which holds"
- file: .engineering/planning/story/real-ports-run-under-an-awaited-loop.md
  line: 67
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the command `cargo test -p b10x-harness-cli --test <the new file>` is a placeholder and the acceptance never names the file, so it cannot be run as written"
- file: .engineering/planning/story/embedding-guide-recommends-loop-task.md
  line: 50
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the rustdoc bullet joins two independent outcomes and gives a command only for the doctest, so the links are unchecked"
```
