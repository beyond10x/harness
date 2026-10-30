---
format: aep.planning-md/3
id: story:bridge-mode-interrupt-cases-are-deterministic
kind: story
status: active
title: bridge_mode's interrupt cases do not depend on machine load
summary: Three bridge_mode rate-threshold cases failed under load and passed on rerun on 2026-10-20; make them deterministic or measured.
relations:
- serves: vision:b10x-owns-its-loop
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-10-30T17:39:19Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-30T17:39:19Z", actor: "human:timo", revision: 3}
---
## Context

`crates/harness-cli/tests/bridge_mode.rs` holds rate-threshold cases that fail under machine load and
pass on rerun of the same binary. Observed on 2026-10-20, each in a unit whose change does not reach
bridge mode:

| case | failure | unit |
|---|---|---|
| `an_interrupt_is_answered_before_the_terminal_frame_when_the_model_client_will_not_build` (`bridge_mode.rs:963`) | "16 of 20 attempts were served correctly, under the 17 a correct server reaches" | cwd adversary, 1 of 3 runs |
| `an_interrupt_that_cancelled_a_turn_decides_its_status_even_when_the_turn_errors` | "16 of 20 attempts …, under the 17" | run U2 implementor; rerun 28 of 28 |
| a `bridge_mode` rate case | "17 of 20 attempts, needs 18" | scope U4 implementor; rerun 417 of 417 |

A gate that goes red without a defect teaches people to rerun until green, which is how a real
regression gets waved through.

## Outcome

`bridge_mode`'s interrupt cases pass or fail on the server's behaviour, not on how loaded the machine
is.

## Acceptance

- Each rate-threshold case is replaced by a deterministic one (an ordering the server guarantees,
  observed per attempt), or its threshold is derived from a measured distribution with the source of
  the numbers in the test; a red run names the attempt that failed and why.
- 20 consecutive runs of `cargo test -p b10x-harness-cli --locked --test bridge_mode` under a parallel
  workspace build pass.

## Scope

- `crates/harness-cli/tests/bridge_mode.rs`; the app-server only if a race the tests expose is real.

## Out of scope

- Bridge-mode behaviour itself, unless the investigation finds a real race.
