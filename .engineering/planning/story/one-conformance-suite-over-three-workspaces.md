---
format: aep.planning-md/1
id: story:one-conformance-suite-over-three-workspaces
kind: story
status: implemented
title: One suite answers what a workspace admits, for all three implementations
relations:
- derived_from: epic:embedded-by-a-consumer
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Evidence

- `ROADMAP.md:168-170` — "What is **not** done is the shared conformance suite: each provider has its own tests, and nothing runs one suite against all three."
- `ROADMAP.md:193-195` — the exit: "`harness-cli` names a workspace implementation and publishes what it admits, with no `cfg` and no branch on which one it got; **the three implementations share one conformance suite**; and `ToolPort` has one implementation".
- `ROADMAP.md:181-191` — the three and what each is for: non-confined, substrate as a library, substrate over a socket; "one trait means the toolset is computed once from what the chosen workspace admits, and an embedder that passes a remote gets the same answer for the same reason".
- `crates/harness-tools/src/local.rs:1096` — `impl Operations for LocalOperations`.
- `crates/harness-tools/src/operations.rs:371` — `impl Operations for Split`.
- `crates/harness-substrate/src/tools.rs:138` — `impl Operations for ConfinedOperations`.
- `STATUS.md:58-59` — the tests each has today: `harness-substrate` 51 unit plus 5 embedded-live, `harness-tools` 80 unit; there is no shared suite between them.

## Context

Two of the three exit conditions for the workspace trait are met: the publication gate lives in one
place and `ToolPort` has one implementation. The third is not, and it is the one an embedder depends
on — the guarantee that handing in a remote workspace gives the same answers as handing in a local
one, rather than the answers whichever crate's own tests happened to cover.

The differences that would show up are the ones already found by hand once each: a path that resolves
differently, a read that is bounded differently, a `run` whose output is cut at a different place.

## Acceptance

One suite runs against `LocalOperations`, `ConfinedOperations` and `Split` from `scripts/gate.sh`,
and a behaviour that differs between them fails it by name.
