---
format: aep.planning-md/3
id: review-result:awaitable-scope-round-1
kind: review-result
status: active
title: Scope critic, round 1, awaitable loop
relations:
- reviews: epic:loop-awaitable-from-async-programs
- reviews: story:loop-task-awaits-a-run
- reviews: story:real-ports-run-under-an-awaited-loop
- reviews: story:embedding-guide-recommends-loop-task
revision: 1
---
needs-revision

story:real-ports-run-under-an-awaited-loop — its acceptance "Each `McpTools` and embedded backend is constructed inside the async test body, on a runtime thread, to show construction is legal there too" asks for something the epic says is illegal and puts out of scope. `McpTools` construction calls `runtime.block_on(...)`, which panics on a runtime thread; making it legal means removing the private runtimes, which the epic excludes. — crates/harness-mcp/src/lib.rs:116-117,128-129

11 promises extracted from the epic (Outcome 4, D1–D5 5, Done When 2 story-dependent); all 11 trace to a story; no two stories claim the same outcome; nothing drafted the epic excludes except the finding above. Out of lane: the Outcome's "with no thread or runtime of its own" against D2's dedicated thread; the "no API break" promise has no named test.

```findings
- file: crates/harness-mcp/src/lib.rs
  line: 117
  category: scope
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "story:real-ports-run-under-an-awaited-loop requires McpTools and the embedded backend to be constructed inside the async test body to show construction is legal, but McpTools::new calls runtime.block_on, which panics on a runtime thread, and removing the private runtimes is excluded by the epic's Out of scope; the acceptance must construct off the runtime thread or record the limit"
```
