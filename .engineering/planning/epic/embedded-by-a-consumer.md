---
format: aep.planning-md/1
id: epic:embedded-by-a-consumer
kind: epic
status: proposed
title: A consumer embeds the loop as a library
summary: Phase 5's adapter, the workflow walked through the library, and one conformance suite over the three workspace implementations.
relations:
- decomposes: initiative:driven-from-outside
revision: 4
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

## What is already closed — 2026-09-15

Recorded during triage of the draft backlog (ORG-0201). *Done When* has two clauses; one is met.

- **"one suite runs against all three workspace implementations"** — met.
  `story:one-conformance-suite-over-three-workspaces` is `implemented`, the suite is
  `crates/harness-substrate/tests/conformance.rs`, and `gate()` in
  `crates/harness-xtask/src/main.rs:81-88` runs it explicitly
  (`cargo test -p b10x-harness-substrate --locked --test conformance`) as well as through the
  workspace run above it.
- **"a library caller has run a turn and a flow"** — open. `story:direct-provider-adapter` and
  `story:workflow-run-through-the-library` carry the two halves.

Two pins the suite left standing are also open under this epic:
`story:one-spelling-of-a-path-in-every-workspace` (`crates/harness-substrate/tests/conformance.rs:1133`)
is still red-when-changed rather than closed, while `story:a-confined-write-makes-its-own-parents`
is `implemented`.

The Evidence section above is stale in one respect: `STATUS.md:29` no longer says "not started" —
see `story:direct-provider-adapter`.
