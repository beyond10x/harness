---
format: aep.planning-md/3
id: review-result:adversary-d288-u2-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave d288 U2 (bridge-mode interrupts)
relations:
- reviews: story:bridge-mode-interrupt-cases-are-deterministic
- reviews: story:bridge-interrupt-start-window-under-load
revision: 1
---
Adversary pass 1 on U2 (`story:bridge-interrupt-start-window-under-load`,
`story:bridge-mode-interrupt-cases-are-deterministic`), wave d288, unit `ac3e0fa`, cases `986007a` on
`adv/d288-u2`. The adversary's report, as returned (scratch paths shortened to `adv-u2/`):

unit: U2-adversary-1, merge ac3e0fa on wave/2026-10-30-d288 (tree head 0580418); my cases are commit 986007a
verdict: CONFIRMED
cases: executed 27→29 (b10x-harness-app-server package; bridge_mode 29→29, unchanged), red 1
origin: introduced 0 / pre-existing 2 / undecided 2
wrote-outside-worktree: 1 directory: adv-u2/
needs-coordinator: no

Cases (`crates/harness-app-server/tests/adversary_d288_u2.rs`):
- `adv_d288_u2_stop_and_resend_while_a_turn_streams_runs_the_next_turn` (:393): red now and at base
  f50af47. Held at the first delta of a streaming turn, `turn/interrupt` 99 and `turn/start` 51 are
  decoded; the drain answers 99 and refuses 51 ("`turn/start` cannot be served while a turn is
  running"), then turn 50 ends `interrupted`.
- `adv_d288_u2_an_interrupt_pipelined_behind_the_second_turn_is_carried_into_it` (:297): green; red on a
  copy with `lib.rs:611` `self.clear_active();` deleted.

Suite: `cargo test -p b10x-harness-app-server --locked --no-fail-fast`: lib 22, adversary_d288_u2 1
passed 1 failed, contract 5; EXIT=101. fmt 0, clippy clean. bridge_mode under a nice -n 19 -j 4 load:
10 of 10 runs, 29 passed each.

Sweep strength (probe copy of the 4000-attempt sweep, counting misses): fixed 13 runs, 0 misses; pre-fix
base 24 runs, 0 to 8 misses, 1 run with 0; `clear_active` deleted 23 runs, 1 to 11 misses, none with 0.

Findings:
- F1 `crates/harness-app-server/src/session.rs:64`, CONFIRMED, pre-existing: `drain_control` reads the
  queue until it is empty, answering the interrupt and refusing the `turn/start` behind it. Reached by a
  client that sends stop and a new prompt within one gap between streamed events.
- F2 `crates/harness-app-server/src/lib.rs:611`, NEEDS-CHANGE, undecided: only the probabilistic sweep
  guards the clear-before-settle fix; a deterministic guard needs a test hook between the loop's end and
  the clear.
- F3 `crates/harness-cli/tests/bridge_mode.rs:1755`, CONFIRMED, undecided: the sweep's comment says its
  100 µs covers the end of the turn; both orders occur from 25 ns to 100 µs.
- F4 `crates/harness-app-server/src/lib.rs:399`, INFEASIBLE, pre-existing: the server ignores the
  `threadId` and `turnId` the pinned trace sends with `turn/interrupt`; no client is known to send
  another thread's ids.

```findings
- file: crates/harness-app-server/src/session.rs
  line: 64
  category: concurrency
  severity: warning
  verdict: CONFIRMED
  origin: pre-existing
  message: drain_control empties the queue mid-stream, so a turn/start sent right behind an interrupt is refused for a turn the same drain just cancelled (red at adversary_d288_u2.rs:448, also at base f50af47)
- file: crates/harness-app-server/src/lib.rs
  line: 611
  category: mutant
  severity: warning
  verdict: NEEDS-CHANGE
  origin: undecided
  message: only the probabilistic sweep guards the clear-before-settle fix, and a replica of that sweep passed the pre-fix server in 1 of 24 runs; a deterministic guard needs a test hook between the loop's end and the clear
- file: crates/harness-cli/tests/bridge_mode.rs
  line: 1755
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: undecided
  message: the sweep's comment says its 100 microsecond delay covers where the turn ends, but both orders occur from 25 ns to 100 us, so scheduler noise gives the coverage and the case never asserts it saw both orders
- file: crates/harness-app-server/src/lib.rs
  line: 399
  category: contract-drift
  severity: note
  verdict: INFEASIBLE
  origin: pre-existing
  message: the server ignores the threadId and turnId that the pinned trace sends with turn/interrupt, so an interrupt naming another thread or a nonexistent turn cancels the running turn; no client is known to send one
```

## Coordinator routing

- F1 pre-existing, but on the surface this unit owns and with a red case already written: fixed in the
  wave as U2 correction 1 rather than filed (the stories' outcome is that the interrupt cases decide on
  the server's behaviour, and this is one of them).
- F2 undecided, decided by the coordinator: in the wave, a test-only seam between the loop's end and
  the clear, so the fix has a deterministic guard.
- F3: the comment is corrected in the same correction.
- F4: no change; nothing found that reaches it.
