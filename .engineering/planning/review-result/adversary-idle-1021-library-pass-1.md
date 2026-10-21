---
format: aep.planning-md/3
id: review-result:adversary-idle-1021-library-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:workflow-run-through-the-library (wave idle-1021 at a9758870)
relations:
- reviews: story:workflow-run-through-the-library
revision: 1
---
idle-1021 adversary pass 1 over the merged wave (detached a9758870, tree harness-w1021-adv), library unit
(merge a9758870): no red case; 2 judgement findings, fixed by documentation at 5fb1e839.

Added `crates/harness-workflow/tests/adversary_walk.rs` (4 cases, green, kept): a walk under `DenyAll`,
a node scope, context with no workspace, and a context symlink pointing out of the workspace.

Findings:

1. J1 `crates/harness-workflow/src/lib.rs` `Walk` (note) — the walk owns the step layer of the
   catalogue it is lent; a caller could expect to narrow one step from outside. Documented: a caller
   narrows the whole walk with `Catalogue::scoped`.
2. J2 `crates/harness-workflow/src/lib.rs` `StepTools` (note) — nothing ties `catalogue_mut` to the
   catalogue the port judges calls by. Documented as a requirement on the implementor.

`crates/harness-cli/tests/workflow.rs` 26/26 unchanged; full `cargo xtask gate` at 5fb1e839 green,
EXIT=0.
