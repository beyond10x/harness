---
format: aep.planning-md/3
id: review-result:awaitable-acceptance-round-1
kind: review-result
status: active
title: Acceptance critic, round 1, awaitable loop
relations:
- reviews: epic:loop-awaitable-from-async-programs
- reviews: story:loop-task-awaits-a-run
- reviews: story:real-ports-run-under-an-awaited-loop
- reviews: story:embedding-guide-recommends-loop-task
revision: 1
---
needs-revision

- story:loop-task-awaits-a-run — the last acceptance line runs `cargo test -p b10x-harness-loop -p harness-conformance`, but the package is named `b10x-harness-conformance`, so cargo rejects the command — `.engineering/planning/story/loop-task-awaits-a-run.md:69` (`crates/harness-conformance/Cargo.toml:2`)
- story:loop-task-awaits-a-run — "the boundary runs every `run-*` and `call-*` scenario ... through `LoopTask` as well as through `AgentLoop`" names no output, count or failure that shows the second path ran or that a divergence fails the suite; `boundary::execute` returns one JSON value per command — `.engineering/planning/story/loop-task-awaits-a-run.md:66` (`crates/harness-conformance/src/boundary.rs:70-82`)
- story:real-ports-run-under-an-awaited-loop — constructing `McpTools` (`Runtime::block_on` at `crates/harness-mcp/src/lib.rs:117`, `:129`) and `Embedded` (owns a `Runtime` that panics when dropped on a runtime thread) inside the async test body cannot pass; the acceptance names neither the change to make it legal nor a refusal to expect — `.engineering/planning/story/real-ports-run-under-an-awaited-loop.md:54`
- story:real-ports-run-under-an-awaited-loop — the Outcome lets a non-`Send` port be "made `Send` or the test records why it cannot be", but the Acceptance names no observable form of that record — `.engineering/planning/story/real-ports-run-under-an-awaited-loop.md:28`
- story:real-ports-run-under-an-awaited-loop — the embedded tests skip with a bare `return;` and print nothing (`crates/harness-substrate/tests/embedded_live.rs:351-355`, `:385-388`), and file operations run on any host with no cgroup (`embedded_live.rs:15-30`); a skipped test passes vacuously — `.engineering/planning/story/real-ports-run-under-an-awaited-loop.md:50`
- story:embedding-guide-recommends-loop-task — "Each code block compiles as a doctest or is copied from a test that compiles" is not checkable: nothing compiles code in `website/docs` (`crates/harness-docs/src/lib.rs:149`) and "copied from" has no comparison step — `.engineering/planning/story/embedding-guide-recommends-loop-task.md:41`
- story:embedding-guide-recommends-loop-task — "the website build in `cargo xtask gate`" names a step that does not exist; `gate()` in `crates/harness-xtask/src/main.rs:77-129` runs no website build (the only one is `.github/workflows/pages.yml:36`) — `.engineering/planning/story/embedding-guide-recommends-loop-task.md:45`
- story:embedding-guide-recommends-loop-task — the Outcome promises that `resume_approval` and `call` are documented as the no-runtime entry and that the `LoopTask` thread is named as interim, but no acceptance item checks either — `.engineering/planning/story/embedding-guide-recommends-loop-task.md:26`

```findings
- file: .engineering/planning/story/loop-task-awaits-a-run.md
  line: 69
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the command `cargo test -p b10x-harness-loop -p harness-conformance` names a package that does not exist (the package is b10x-harness-conformance), so the item can never pass"
- file: .engineering/planning/story/loop-task-awaits-a-run.md
  line: 66
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the conformance item names no output, count or failure that shows the boundary ran the LoopTask path or that a divergence from AgentLoop fails the suite"
- file: .engineering/planning/story/real-ports-run-under-an-awaited-loop.md
  line: 54
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "constructing McpTools (block_on at harness-mcp/src/lib.rs:117,129) and Embedded (owns a Runtime) inside the async test body panics per the epic's own evidence; the acceptance names neither the change that makes it legal nor the refusal to expect"
- file: .engineering/planning/story/real-ports-run-under-an-awaited-loop.md
  line: 28
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Outcome allows a non-Send port to be recorded instead of made Send, but the Acceptance names no observable form of that record, so the named tests may be absent and the story still closes"
- file: .engineering/planning/story/real-ports-run-under-an-awaited-loop.md
  line: 50
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the existing embedded tests skip with a silent return (embedded_live.rs:351-355, 385-388) and file operations need no cgroup, so 'skipped with a printed reason' matches nothing in the tree and a skip passes vacuously"
- file: .engineering/planning/story/embedding-guide-recommends-loop-task.md
  line: 41
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "'compiles as a doctest or is copied from a test that compiles' is unobservable: nothing compiles code in website/docs and 'copied from' has no comparison step"
- file: .engineering/planning/story/embedding-guide-recommends-loop-task.md
  line: 45
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "'the website build in cargo xtask gate' names a step that does not exist; gate() in crates/harness-xtask/src/main.rs runs no website build"
- file: .engineering/planning/story/embedding-guide-recommends-loop-task.md
  line: 26
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the Outcome promises that resume_approval and call are documented as the no-runtime entry and that the LoopTask thread is named as interim, but no acceptance item checks either"
```
