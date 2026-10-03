---
format: aep.planning-md/3
id: story:direct-provider-adapter
kind: story
status: proposed
title: A consumer's adapter binds the loop's ToolPort to its own operations
relations:
- derived_from: epic:embedded-by-a-consumer
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: STATUS.md
- confidence: inferred
  path: crates/harness-loop/src/environment.rs
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-09-15T17:26:41Z", actor: "human:timo", revision: 4, imported: true}
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

A consumer binds TurnEnvironmentProvider to its operations, completes a direct-provider run through the library, retains the environment revision used by every turn, passes its own lifecycle conformance, and supplies attributable evidence for STATUS.md's embedding row (ROADMAP.md Phase 5 exit). The consumer owns its adapter; this repository changes only proven gaps in its public library ports and never imports a sibling checkout.

## Since this was drafted — 2026-09-15

The original Evidence section is historical: runtime/agent is not the current consumer and embedding is no longer unstarted. ROADMAP.md Phase 5 names Agent Platform, the existing TurnEnvironmentProvider seam and per-turn revision evidence. The acceptance now follows that exit. STATUS.md's consumer observations remain attributed observations, not verification from this repository. No consumer repository or live integration was rerun in this planning review.
