---
format: aep.planning-md/1
id: story:embedded-exec-exercised
kind: story
status: implemented
title: The embedded driver runs a confined program on a machine, once
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
revision: 8
---
## Evidence

What discharges this story, at `f470a76`:

- `STATUS.md:30` — *Substrate confinement*: "On 2026-08-31 an authorized Anthropic run in an
  embedded delegated scope used the admitted Go toolchain to build, format, test and vet a scratch
  todo server and frontend; the compiled app's frontend, health route and CRUD API then passed
  independent host checks." The row's *next evidence* column now asks to "repeat the end-to-end
  exercise through a socket deployment", not the embedded one.
- `README.md:51` — "working, including execution, and `run` has been *exercised* against a confined
  process: on 2026-08-31 an embedded delegated scope built, formatted, tested and vetted a Go server
  through the admitted toolchain; see `STATUS.md`".
- `website/docs/status.md:43-47` — the same paragraph for an outside reader, with the limit stated:
  "This is evidence for that live route and confinement path, not a promotion of the pinned wire
  fixtures."
- `ee7a631` (2026-08-31 11:01:35 +0200, the `chore(release): 0.7.0` commit; tag `0.7.0` at `18fde7c`) — the commit that added that sentence to
  `STATUS.md` and to the public status page, and removed the "the embedded driver's exec is still
  unexercised on this machine" clause in the same diff. Reachable on `origin/main`.
- `story:go-toolchain-in-confined-runs` (implemented) — the admitted Go toolchain the run used.

What the story was drafted against, and which no longer reads that way:

- `STATUS.md` at `1b1f6a8` (2026-08-29) — "the embedded driver's exec is still unexercised on this
  machine, which needs the same delegated scope"; removed at `ee7a631`.
- `docs/reviews/2026-08-29-code-review-2.md:38` — "Still open: the embedded driver's exec is
  unexercised on this machine (needs the same delegated scope)". A dated review, left as it was.
- `crates/harness-substrate/tests/embedded_live.rs:346-356` — the guard is unchanged at HEAD:
  `a_staged_driver_is_a_program_the_confined_run_can_actually_start` returns early when
  `B10X_CGROUP_ROOT` is unset, so the gate still says nothing about this path. No run of that test
  under a delegated scope is recorded anywhere in this repository.

## Context

The embedded driver is the path `--substrate-embedded` uses, and it is the one an embedder gets for
free with no deployment. When this story was drafted its exec had never run. The test that would
prove it exists and is guarded by an environment variable, which is the right shape — but the
guard's absence produces a pass, so a green gate says nothing about this path either way.

The socket path was in the same position until 2026-08-29, and when it was finally run against a real
daemon it turned out to have four client defects behind each other (`docs/reviews/2026-08-29-code-review-2.md:36`).
That is the prior for this path — and on 2026-08-31 the embedded path was run anyway, by a different
and larger exercise than the one this story first asked for: an authorized model run inside a
delegated scope that drove a whole Go build-and-test cycle through the admitted toolchain, checked
afterwards from the host.

**What this acceptance deliberately no longer requires (2026-09-15).** The first wording named one
test — `a_staged_driver_is_a_program_the_confined_run_can_actually_start` — running to its assertions
under a delegated cgroup scope. That run never happened and is recorded nowhere; the evidence that
exists is the 2026-08-31 exercise, which is stronger on the question this story asks (does the
embedded driver start a real program under confinement) and silent on a different one (does that
particular test's staging path work). The acceptance was re-worded to the evidence produced. The
narrower question stays open as gate hygiene: the guard at
`crates/harness-substrate/tests/embedded_live.rs:351` still turns an unset `B10X_CGROUP_ROOT` into a
pass, and `epic:gate-stays-trustworthy` is where that belongs, not here.

## Acceptance

One exec has gone through the **embedded** backend on a real machine inside a delegated cgroup
scope, and this repository states it as a dated observation instead of an open gap. Five
observables, each readable in the tree:

1. `STATUS.md`'s *Substrate confinement* row names the date, the workload and the check: an
   authorized run on **2026-08-31**, in an embedded delegated scope, that used the admitted Go
   toolchain to build, format, test and vet a scratch todo server and frontend, whose frontend,
   health route and CRUD API then passed independent host checks.
2. The same row no longer says the embedded driver's exec is unexercised, and its *next evidence*
   column asks for the **socket** deployment rather than the embedded one.
3. `README.md`'s substrate-confinement row says `run` has been *exercised* against a confined
   process, with that date, rather than "published, not yet exercised".
4. The published status page tells an outside reader the same thing (`website/docs/status.md`).
5. The sentence is reachable on `origin/main`, so the claim is dated by a commit rather than by a
   session.
