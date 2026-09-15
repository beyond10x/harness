---
format: aep.planning-md/1
id: story:direct-provider-adapter
kind: story
status: proposed
title: A consumer's adapter binds the loop's ToolPort to its own operations
relations:
- derived_from: epic:embedded-by-a-consumer
- serves: vision:b10x-owns-its-loop
revision: 4
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

## Since this was drafted — 2026-09-15

**The consumer named in the Evidence above is the wrong one, and "not started" is no longer true.**
Recorded during triage of the draft backlog (ORG-0201); the story is kept because the work it asks
for is still open, not because its premise survived.

- `ROADMAP.md:181` — "The first consumer is **agent-platform**, not `runtime/agent`."
  `ROADMAP.md:179` states the phase as "begun — the first embedder exists; the binding and the live
  pin are open."
- `STATUS.md:29` — the Embedding row no longer reads "not started". It records Agent Platform as the
  first embedder, pinning `b10x-harness-wire`, `b10x-harness-loop` and `b10x-harness-messages` at
  tag `0.10.0`, two releases behind this tree — read in a sibling checkout on 2026-09-15 and
  explicitly **not verified from this repository**.
- The seam the embedder is asked to bind is now named and exists here:
  `TurnEnvironmentProvider` (`STATUS.md:18`, `ROADMAP.md:191`).

So the acceptance below should be read against `ROADMAP.md:201`'s exit — an embedder binds
`TurnEnvironmentProvider` and retains per-turn revision evidence — rather than against a
`runtime/agent` adapter that was never built. Rewriting the acceptance is a decision about what this
story now asks for, and is left to whoever picks it up.

Not verified here: anything inside the embedder's own repository. This triage read only this tree.
