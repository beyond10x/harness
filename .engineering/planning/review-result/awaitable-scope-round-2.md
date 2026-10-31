---
format: aep.planning-md/3
id: review-result:awaitable-scope-round-2
kind: review-result
status: active
title: Scope critic, round 2, awaitable loop
relations:
- reviews: epic:loop-awaitable-from-async-programs
- reviews: story:loop-task-awaits-a-run
- reviews: story:real-ports-run-under-an-awaited-loop
- reviews: story:embedding-guide-recommends-loop-task
revision: 1
---
approve

Round 1's finding is resolved: `story:real-ports-run-under-an-awaited-loop` builds each port inside the `LoopTask` builder, on the task's thread. 11 promises of the revised epic (Outcome, D1–D6, Done When) each trace to a story; no two stories claim one outcome; nothing drafted lands in the epic's exclusions.

Out of lane: no acceptance for D3's ordering or the sink coming back, for D6's "does not block", or for the `LoopTask::spawn` signature; the real-ports story's summary still said "Send gaps fixed or recorded".

```findings
[]
```
