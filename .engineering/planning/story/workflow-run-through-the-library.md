---
format: aep.planning-md/1
id: story:workflow-run-through-the-library
kind: story
status: draft
title: A workflow is walked through the library, with no b10x-harness process
relations:
- derived_from: epic:embedded-by-a-consumer
revision: 2
---
## Evidence

- `ROADMAP.md:283-285` — Phase 8 exit: "the shipped binary walks `adp-default.projected.yaml` end to end over both emulators, takes one retreat and stops at its bound, and puts every transition to a hook program that refuses one of them — with no `metaharness` and no `protocol` process alive; **and one embedded run under Phase 5's consumer does the same through the library**".
- `crates/harness-cli/tests/workflow.rs:1120-1176` — `the_projected_adp_workflow_walks_end_to_end`, over both wires, on the unedited projected document; the binary half of that exit is reached.
- `crates/harness-cli/tests/workflow.rs:437`, `:569`, `:650` — a transition hook that refuses a leave, refuses an enter, and one that cannot answer failing closed.
- `ROADMAP.md:236-242` — why the library half is not optional: "A driver that is a process tree of `protocol drive` → `metaharness` → `b10x-harness` per step cannot be embedded".
- `crates/harness-cli/src/workflow.rs` — the `StepRunner` that binds a step to a turn lives in the binary's crate, not in `harness-flow`.

## Context

Phase 8's exit has two halves and only one is reached: the binary walks the flow, over both
emulators, taking a retreat and answering a refusing governor. The other half is an embedder walking
the same document through the library — which is the whole argument for putting the runner here
rather than leaving it a process-per-step driver.

Today the step runner that turns a step into a turn is in `harness-cli`, so an embedder cannot reach
it without the binary. Whether that moves, or the embedder writes its own, is the design question
this story carries.

## Acceptance

A library caller walks `adp-default.projected.yaml` to `flow-finished`, takes one retreat and stops
at its bound, with no `b10x-harness`, `metaharness` or `protocol` process alive during the walk.
