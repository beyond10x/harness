---
format: aep.planning-md/1
id: story:embedded-exec-exercised
kind: story
status: draft
title: The embedded driver runs a confined program on a machine, once
relations:
- derived_from: epic:measured-not-emulated
revision: 2
---
## Evidence

- `STATUS.md:23` — "exec has been *exercised* over the socket (2026-08-29: `/bin/echo` and a 12 s `/bin/sleep` through `run`, cgroup-confined); **the embedded driver's exec is still unexercised on this machine**, which needs the same delegated scope".
- `docs/reviews/2026-08-29-code-review-2.md:38` — "Still open: the embedded driver's exec is unexercised on this machine (needs the same delegated scope)".
- `crates/harness-substrate/tests/embedded_live.rs:163-171` — `a_staged_driver_is_a_program_the_confined_run_can_actually_start` returns early when `B10X_CGROUP_ROOT` is unset, so on a machine without a delegated subtree it reports success having exercised nothing.
- `crates/harness-substrate/tests/embedded_live.rs:200-215` — and again when the named subtree is not one this process sits inside: `facts.confines_execution()` false, early return, then the exec.
- `STATUS.md:24` — what the socket path needed to work: a delegated user scope, `systemd-run --user --scope -p "Delegate=cpu memory pids"`, with the daemon moved into a child cgroup.
- `README.md:42` — "substrate confinement, embedded | working, including execution — but `run` has been *published*, not yet *exercised* against a confined process".

## Context

The embedded driver is the path `--substrate-embedded` uses, and it is the one an embedder gets for
free with no deployment. Its exec has never run. The test that would prove it exists and is guarded
by an environment variable, which is the right shape — but the guard's absence produces a pass, so a
green gate says nothing about this path either way.

The socket path was in the same position until 2026-08-29, and when it was finally run against a real
daemon it turned out to have four client defects behind each other (`docs/reviews/2026-08-29-code-review-2.md:36`).
That is the prior for this path.

## Acceptance

`a_staged_driver_is_a_program_the_confined_run_can_actually_start` runs to its assertions under a
delegated cgroup scope on a real machine, and `STATUS.md`'s substrate row names the date it did
instead of naming it as unexercised.
