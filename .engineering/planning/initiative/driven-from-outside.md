---
format: aep.planning-md/1
id: initiative:driven-from-outside
kind: initiative
status: draft
title: Something other than this repository's own tests drives the loop
summary: The bridge proof, the first embedder, and one answer about what a workspace admits for all three implementations.
relations:
- serves: vision:b10x-owns-its-loop
revision: 2
---
## Evidence

- `STATUS.md:17` — bridge mode, next evidence: "**run `runtime/agent`'s real bridge against this binary.** Everything so far is this component's own client, written from the bridge's source; the two processes have never spoken, and no gate compares the two inventories".
- `STATUS.md:22` — embedding: "**not started.** Nothing embeds this component yet"; next evidence is a `runtime/agent` direct-provider adapter binding `ToolPort` to its capability compiler.
- `ROADMAP.md:42-44` — Phase 2 exit: "the existing bridge, pointed at this binary instead of `codex`, drives a turn".
- `ROADMAP.md:145-155` — Phase 5: the first consumer, "the first time the tools are real operations".
- `ROADMAP.md:168-170` — Phase 6 residue: "What is **not** done is the shared conformance suite: each provider has its own tests, and nothing runs one suite against all three."
- `ROADMAP.md:190-191` — why that suite belongs to this initiative: "an embedder that passes a remote gets the same answer for the same reason".
- `ROADMAP.md:285` — Phase 8 exit's second half: "one embedded run under Phase 5's consumer does the same through the library".
- `AGENTS.md:105-109` — invariant 15: bridge-mode method inventories are a copy, nothing here checks the copy against the original, "and the only thing that catches a mismatch is running the real bridge".

## Context

Every suite this repository runs is one it wrote for itself: the emulators, the end-to-end binary
tests, its own bridge client. That is enough to prove the component agrees with itself and cannot
prove it agrees with anyone. Three things would change that, and they are the same thing at three
distances: a real bridge client speaking to `app-server`, a real embedder holding `AgentLoop` as a
library, and one conformance suite that all three `Operations` implementations must satisfy.

Two of the three are cross-repository and cannot be finished here alone; that is a scheduling fact
about them, not a reason to leave them out of the plan, because `STATUS.md` already names them as
what the two areas are waiting for.

## Done When

A process this repository does not own has driven the loop — over the bridge, or as a library — and
the run is retained as evidence.
