---
format: aep.planning-md/1
id: story:workflow-run-panics-and-drops-its-profile
kind: story
status: implemented
title: workflow run panics where every other verb refuses, and ignores the profile it was given
summary: dispatch routes Command::Workflow past apply_profiles, so the run panics at exit 101 with no endpoint and -p is accepted and dropped.
relations:
- derived_from: epic:pinned-interfaces-honest
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Evidence

Found by the implementor of `story:argv-pin-carries-effective-defaults` on 2026-08-30, while building
an invocation from the pinned CLI document. Outside that story's acceptance, so it was reported
rather than fixed.

- `b10x-harness workflow run --flow <valid> --input hi` with no endpoint configured **panics**:
  `panicked at crates/harness-cli/src/lib.rs:1015: "apply_profiles fills the endpoint or refuses the
  run"`, exit 101.
- `crates/harness-cli/src/lib.rs:2957` — `dispatch` routes `Command::Workflow` straight to
  `workflow::dispatch`, which never calls `apply_profiles`. The expectation the panic message states
  is therefore never established on that path.
- **`-p/--profile` on `workflow run` is accepted and silently ignored** for the same reason, and it
  is a flag `contracts/cli/b10x-harness/2026-08-30.2/argv.json` pins — so the document promises a
  consumer something the binary does not do.
- `story:missing-model-refuses-by-name` is implemented and pins that a run with no model refuses by
  name on every machine and never panics. This is the same requirement on the verb that story did
  not cover.

## Context

Two defects with one cause. The panic is the visible one and the silently-ignored profile is the
worse one: a run that names `-p prod` and gets none of it produces a result nobody can reproduce
from the flags, which is the failure the profile design exists to prevent.

`AGENTS.md` invariant 9 is about refusals the model must learn about; this is the operator-facing
counterpart, and `story:missing-model-refuses-by-name`'s own acceptance — "refuses by name on every
machine, never panics" — is the sentence this path breaks.

## Acceptance

`workflow run` with no endpoint and no configured provider refuses by name and exits 1, never 101;
and a `-p/--profile` named on `workflow run` either applies or is refused, never accepted and
ignored.

## Note on cost

The fix needs `Clone` on `WorkflowRunOptions` so `apply_profiles` can run on that path. That is a
behaviour change — profiles start applying to `workflow run`, which they never have — so it needs its
own tests and its own changelog entry, and it is why the story that found it did not take it.
