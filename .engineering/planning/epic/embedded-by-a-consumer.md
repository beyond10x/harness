---
format: aep.planning-md/1
id: epic:embedded-by-a-consumer
kind: epic
status: draft
title: A consumer embeds the loop as a library
summary: Phase 5's adapter, the workflow walked through the library, and one conformance suite over the three workspace implementations.
relations:
- decomposes: initiative:driven-from-outside
revision: 2
---
## Evidence

- `STATUS.md:22` — embedding: "**not started.** Nothing embeds this component yet"; next evidence "a `runtime/agent` direct-provider adapter binding `ToolPort` to its capability compiler".
- `ROADMAP.md:145-155` — Phase 5, not started: the adapter, "the first consumer, and the first time the tools are real operations", plus one authorized live run kept as `vendor_live`.
- `ROADMAP.md:157-170` — Phase 6, done under another name, except: "nothing runs one suite against all three".
- `ROADMAP.md:190-195` — the exit that is still open: "the three implementations share one conformance suite", so "an embedder that passes a remote gets the same answer for the same reason".
- `ROADMAP.md:283-285` — Phase 8's exit includes "one embedded run under Phase 5's consumer does the same through the library".
- `crates/harness-tools/src/operations.rs:371`, `crates/harness-tools/src/local.rs:1096`, `crates/harness-substrate/src/tools.rs:138` — the three production `impl Operations`, each tested only in its own crate.

## Outcome

A program outside this repository holds `AgentLoop` as a library and gets the same answers about
what a workspace admits, whichever workspace implementation it hands in.

## Scope

The adapter (in the consumer's repository), the workflow walked through the library rather than the
binary, and one conformance suite the three `Operations` implementations must pass.

## Out of Scope

Hosting, an admission transport or a durable store (`AGENTS.md:193-201`).

## Done When

A library caller has run a turn and a flow, and one suite runs against all three workspace
implementations in `scripts/gate.sh`.
