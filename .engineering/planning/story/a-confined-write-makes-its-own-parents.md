---
format: aep.planning-md/1
id: story:a-confined-write-makes-its-own-parents
kind: story
status: draft
title: A write under a directory that does not exist has one answer, not two
summary: A confined run cannot create src/new/mod.rs and the same run without confinement can; pinned by the conformance suite, not closed.
relations:
- derived_from: epic:embedded-by-a-consumer
revision: 1
---
## Evidence

Found by the adversarial pass against `wt/one-conformance-suite-over-three-workspaces` on
2026-08-30, and **pinned rather than closed** by that unit: the conformance suite asserts today's
split so the gap cannot widen unnoticed.

- `crates/harness-substrate/tests/conformance.rs` — `a_write_under_a_directory_that_does_not_exist_yet_is_one_answer_in_every_workspace`
  asserts the current behaviour and names this story in its doc and its assertion message.
- Measured, all three implementations, same tree, same call: `LocalOperations` writes
  `deep/down/new.txt` and answers `{"on_disk":"…","wrote":true}`; `ConfinedOperations` and `Split`
  refuse `resource.not-found` and leave nothing on disk.
- `crates/harness-tools/src/local.rs:1249` — the local provider pins the create-parents behaviour in
  its own test, so it is documented and deliberate on that side.
- `crates/harness-cli/src/lib.rs:2558` — the CLI composes `Catalogue::of(Split::new(reading,
  confined))`. This is what makes the difference **live** rather than latent: a run with
  `--substrate-embedded` cannot create `src/new/mod.rs`, and the same run without it can.

## Context

The conformance suite exists to make one question have one answer across the three workspace
implementations. This is the largest difference it found, and the only one reachable through a
shipped flag rather than through an embedder holding the trait directly.

Creating a file under a directory that does not exist yet is the commonest write a model makes — a
new module, a new test file, a new fixture directory. Under `--substrate-embedded` it fails, and the
failure is a refusal the model must then work around by a route the catalogue does not offer, since
there is no `dir_create` entry.

## Why it was pinned rather than fixed

Neither direction is small, and the unit that found it said so rather than picking one quietly.

- **Make the confined side create parents.** It needs a directory route on `Backend` — five
  operations, two implementations, and a wire contract behind it.
- **Make the local side stop.** It would lose documented, tested behaviour that real runs use
  (`local.rs`, `a_new_file_under_directories_that_do_not_exist_yet_still_writes`).

## Acceptance

A write to a path whose parent directory does not exist has one outcome across `LocalOperations`,
`ConfinedOperations` and `Split` — and whichever outcome is chosen, `crates/harness-substrate/tests/conformance.rs`'s
pinning case is rewritten to assert it rather than the split it asserts today.

## Open question for the operator

Which direction. Creating parents is what a model expects and what the local provider already does;
refusing is what containment argues for, and adding a directory route widens `Backend`'s surface and
the wire contract with it.
