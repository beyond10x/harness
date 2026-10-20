---
format: aep.planning-md/3
id: story:bridge-interrupt-start-window-under-load
kind: story
status: draft
title: The bridge's start-window interrupt test decides the same way under load
summary: 'Filed from adversary pass 1 of wave llm-1: 1 miss in 8 suite runs on a loaded machine; the pin''s role is an unmeasured hypothesis.'
relations:
- serves: vision:b10x-owns-its-loop
- derived_from: story:bridge-interrupt-race-pinned
revision: 2
---
## Context

Filed by the coordinator of wave llm-1 from the adversary's pass-1 finding 8
(`review-result:adversary-u1-pass-1`), origin `undecided`, decided here as not blocking the unit.

`crates/harness-cli/tests/bridge_mode.rs:1216`,
`an_interrupt_is_answered_before_the_terminal_frame_when_the_model_client_will_not_build`,
requires 17 of 20 attempts served correctly. Observed on 2026-10-20, on a machine where other
sessions were building: 1 suite run of 8 returned 16 of 20 (implementor's first baseline run,
`~/.cache/harness-wave-2026-10-20-w01/u1/scratch/test-cli-before.log`); 5 of 5 isolated reruns
and the adversary's suite run passed 20 of 20 or above the threshold. The case's own comment says
a correct server fails it about once in 260,000 runs.

The adversary read the path it drives (`BaseUrl::new` refusal inside `binding()`) as identical on
the spike and on llm 0.7.0, so "the pin narrowed the start window" is a hypothesis nobody measured.
The race is between the pipelined interrupt and `drive_turn`'s terminal frame
(`crates/harness-app-server/src/lib.rs`); the adversary judged a deterministic case infeasible
without an app-server change.

A second case of the same family missed once later the same night, on llm 0.8.0:
`an_interrupt_is_answered_before_the_terminal_frame_even_when_the_turn_errors`, 16 of 20 on a
run where every lane's wall time was 1.5 to 2 times its usual
(`~/.cache/harness-wave-2026-10-20-w01/u1/scratch/c3b-test-cli-nff-2.log`); 28 of 28 on the rerun.
Two cases, one miss each, both under load.

## Outcome

The start-window interrupt test either decides the same way under load (a measured miss rate
below its own threshold over 100 suite runs on a loaded machine) or is made deterministic by an
app-server seam, and the store records which.

## Acceptance

100 consecutive runs of `cargo test -p b10x-harness-cli --test bridge_mode --locked` on a machine
with at least one other build running report 0 misses of the 17/20 threshold, or the case is
rewritten against a seam that makes the interrupt's arrival before the terminal frame a
deterministic event; the chosen path is named in `STATUS.md`'s bridge row.

## Out of scope

The llm pin; `harness-loop`.
