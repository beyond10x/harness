---
format: aep.planning-md/1
id: story:a-confined-write-makes-its-own-parents
kind: story
status: implemented
title: A write under a directory that does not exist has one answer, not two
summary: Local, embedded-confined, and socket-confined workspaces uniformly refuse a write whose parent is absent.
relations:
- derived_from: epic:embedded-by-a-consumer
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 6
---
## Decision

All three workspace implementations refuse a write whose parent directory does not exist, name the absent parent, and leave no filesystem side effect. The alternative would require widening the pinned substrate boundary with a directory-creation operation; this remediation does not silently add that capability.

## Evidence

- `crates/harness-substrate/tests/conformance.rs` exercises local, embedded-confined, and socket-confined adapters over the same case and asserts identical refusal plus absence on disk.
- `crates/harness-tools/src/local.rs` pins the local provider to the same refusal.
- The full `cargo xtask gate` runs the conformance target explicitly as well as through the workspace.

## Acceptance

A write to a path whose parent directory does not exist has one outcome across `LocalOperations`, `ConfinedOperations`, and `Split`: a named refusal, with nothing created.
