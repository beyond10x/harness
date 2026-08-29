---
format: aep.planning-md/1
id: story:direct-provider-adapter
kind: story
status: draft
title: A consumer's adapter binds the loop's ToolPort to its own operations
relations:
- derived_from: epic:embedded-by-a-consumer
revision: 2
---
## Evidence

- `STATUS.md:22` — "Embedding | **not started.** Nothing embeds this component yet | a `runtime/agent` direct-provider adapter binding `ToolPort` to its capability compiler".
- `ROADMAP.md:145-155` — Phase 5, not started: "a `runtime/agent` direct-provider adapter that embeds this loop and binds `ToolPort` to its capability compiler — the first consumer, and the first time the tools are real operations"; exit: "a direct-provider run passes `runtime/agent`'s own lifecycle conformance".
- `ROADMAP.md:236-242` — why an embedder is the point: "an embedder that wants a workflow wants its ordering, its context scope and its retreat *inside* the loop it holds".
- `README.md:16-17` — "The arrow points inward — something else embeds this, never the reverse."
- `AGENTS.md:36-42` — invariant 2: this component may not depend on the sibling that embeds it, so the adapter is that repository's code, not this one's.

## Context

Every tool call this loop has ever served went to a catalogue this repository wrote. The first
embedder replaces that with real operations compiled from someone else's capability model, which is
where the port's shape gets tested rather than assumed.

The work is in the consumer's repository by invariant 2; what belongs here is whatever the adapter
finds missing in the library surface — `AgentLoop::run_in`, `ToolPort`, `ApprovalPort`, `HookPort`,
the `RunLedger` — and the evidence that the run passed the consumer's own lifecycle conformance.

## Acceptance

A direct-provider run driven by the consumer's adapter passes that consumer's lifecycle conformance,
and `STATUS.md`'s embedding row names it instead of saying "not started".
