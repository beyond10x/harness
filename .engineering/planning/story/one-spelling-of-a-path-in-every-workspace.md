---
format: aep.planning-md/1
id: story:one-spelling-of-a-path-in-every-workspace
kind: story
status: draft
title: One spelling of a path names one file in every workspace
summary: ./notes.txt reads locally and is refused as a path escape when confined; the model is never told which spelling is required.
relations:
- derived_from: epic:embedded-by-a-consumer
revision: 1
---
## Evidence

Found by the adversarial pass against `wt/one-conformance-suite-over-three-workspaces` on
2026-08-30, and **pinned rather than closed** by that unit.

- `crates/harness-substrate/tests/conformance.rs` — `a_path_spelled_with_a_leading_dot_names_the_same_file_in_every_workspace`
  asserts the current behaviour and names this story.
- Measured: `./notes.txt` reads as the file through `LocalOperations` and is refused
  `workspace.path-escape` through `ConfinedOperations`.
- `crates/harness-tools/src/operations.rs` — the trait's `file_read` `# Errors` says the path being
  "outside the workspace" is what refuses. `./x` is not outside it.
- `crates/harness-tools/src/catalogue.rs:659` — what the model is told: "Paths are relative to the
  workspace root; `.` is the root". That sentence is on `dir_list`. Nothing tells a model that `./`
  is illegal for `file_read`.

## Context

Two spellings of one path, and they disagree about whether the file exists. The model is never told
which spelling is required, and the one hint it does get — `.` is the root — points the wrong way.

This is smaller than `story:a-confined-write-makes-its-own-parents`: a model reaching it is doing so
by habit rather than by following a documented workflow, and the refusal is legible when it happens.

## Why it was pinned rather than fixed

`crates/harness-substrate/src/backend.rs` refuses this crate the fix in its own words: "A path that
leaves the workspace is refused by substrate and never by this crate: re-implementing containment
here would make two answers to one question." Normalising a spelling in `harness-substrate` is the
first half of exactly that.

The two places it could live:

- **substrate's own path handling**, which is where containment is decided — another repository, and
  a change to what a guarded workspace admits.
- **`harness_tools::Catalogue`**, before any provider sees the path — which changes what every entry
  receives for every embedder, not just this one.

## Acceptance

`./notes.txt` and `notes.txt` name the same file through `LocalOperations`, `ConfinedOperations` and
`Split` — or the catalogue tells the model, on the entries where it matters, which spellings are
admitted. Whichever is chosen, the pinning case in `crates/harness-substrate/tests/conformance.rs` is
rewritten to assert it.
