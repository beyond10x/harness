---
format: aep.planning-md/3
id: review-result:adversary-1104-u4-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave 2026-11-04 U4 (turn/start after an interrupt)
relations:
- reviews: story:bridge-turn-start-after-interrupt-on-every-path
revision: 2
---
unit: U4 story:bridge-turn-start-after-interrupt-on-every-path, unit a158f03; findings cover merge 58c2d1a in ~/.local/state/worktree/trees/b10x/harness/harness-1104-adv-u4 plus the uncommitted test file
verdict: INFEASIBLE (2 red cases, in-order serving broken by the deferred queue on reachable-but-unshown paths); 1 suite gap CONFIRMED
cases: executed 33→36, red 2
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: 1 directory, ~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1104/adv-u4/ (only the assigned scratch)
needs-coordinator: yes, a decision on finding 1 (refuse request 52 or serve it) and on whether to change the code or only the fact-3 doc

**1. Diff.** `git diff --stat` is empty because the only change is a new untracked file. `git status --short`:
```
?? crates/harness-app-server/tests/adversary_1104_u4.rs
```
It is a test file, and no implementation file was touched.

**2. New cases** (`crates/harness-app-server/tests/adversary_1104_u4.rs`). The first run had these cases alone, before any suite run, exit 101:
- `an_interrupt_sent_after_a_held_request_is_not_answered_ahead_of_it`: **red**.
  - Assertion: `request 52 was sent before interrupt 99 and was held in `deferred`, yet `Wire::settle_interrupts` read the channel past it and answered 99 first (52 at frame 14, 99 at frame 12)`.
  - It is green on the base sources (2e37e2e).
- `a_request_held_behind_an_interrupt_is_answered_before_a_later_malformed_frame_ends_the_connection`: **red**.
  - Assertion: `request 51 was sent before the malformed frame, but `Wire::next_frame` served the stashed malformed frame ahead of the held request, so the connection ended with 51 unanswered. Ended: Err("transport: a frame was not JSON")`.
  - It is green on the base sources.
- `requests_held_behind_an_interrupt_are_served_in_the_order_they_were_sent`: **green** now. It catches the wrong-order mutant M2 below.
  - It is red on the base sources, which is expected: the base refuses the held `turn/start`.

**3. Suite run** (after the cases existed), with `--no-fail-fast` and all the brief's test targets, exit 101:
- `--lib`: `test result: ok. 23 passed; 0 failed`
- `turn_start_after_interrupt`: `test result: ok. 3 passed; 0 failed`
- `contract`: `test result: ok. 5 passed; 0 failed`
- `adversary_d288_u2`: `test result: ok. 2 passed; 0 failed`
- `adversary_1104_u4`: `test result: FAILED. 1 passed; 2 failed`
- fmt `--check`: exit 0. Clippy `--all-targets --locked -D warnings`: exit 0.

**Mutation results.** All mutations were made in a scratch copy at `<scratch>/mut`, never in the tree.
- **M1**, `hold_or_refuse` always holds (session.rs:227 `if true {`): the guard case `a_turn_start_sent_before_the_interrupt_while_a_tool_call_is_outstanding_is_still_refused` fails. The two acceptance cases pass, so the guard is the only thing catching M1.
- **M2**, `deferred` drained newest-first (session.rs:228 `push_front`): all 3 of the unit's cases pass. My `requests_held_behind_an_interrupt_are_served_in_the_order_they_were_sent` fails: `51 was sent first and answered at frame 11, 52 at frame 10`.
- **Base**: the crate's `lib.rs` and `session.rs` at 2e37e2e, on the merged workspace. Only U4 touched this crate between base and merge. Cases 1 and 2 above are green there, so both are introduced.

**4. Findings**

| file:line | what was measured | what reaches it | verdict | origin |
|---|---|---|---|---|
| crates/harness-app-server/src/session.rs:140 | `settle_interrupts` reads `reader.next_within` directly and skips `deferred`, so an interrupt sent after a held request is answered first. The held `turn/start` 52 then starts a third turn after that interrupt. If turn 2 had streamed first, its drain would have refused 52, so the outcome depends on timing. The fact-3 doc (:181) lists the readers that serve `deferred` first and leaves out `settle_interrupts`. | Constructed: two requests held behind an interrupt during a tool call, and an interrupt decoded after turn 1 let go. No client was shown sending two requests after an interrupt. | INFEASIBLE | introduced |
| crates/harness-app-server/src/session.rs:248 | `next_frame` serves the stash before `deferred`, so a malformed frame sent after a held `turn/start` ends the connection with that request unanswered. The base answered it with -32601 first. | Constructed: needs a client that sends invalid JSON; none found. | INFEASIBLE | introduced |
| crates/harness-app-server/src/session.rs:228 | The unit's 3 cases pass with `deferred` drained newest-first (M2). Each case holds only one request. | Any two held requests; the new case covers it now. | CONFIRMED | introduced |

Fix for row 1, not applied: in `settle_interrupts`, take `self.take_deferred(control)` before `self.reader.next_within(...)`. This cannot loop forever: `take_deferred` returns nothing once the turn has heard an interrupt, and a turn that has not heard one refuses the request instead of holding it again. If the coordinator wants request 52 served rather than refused, the drain path has to change instead; either way the fact-3 doc has to match the code.

Fix for row 2, not applied: have `next_frame` take `deferred` before the stash. Fact 3 says the timing of stashed non-requests is harmless.

**5. Attacked and not broken**
- **Memory**: `deferred` only takes frames out of the reader's channel, which is itself unbounded (`channel()`), so there is no new way for a client to grow memory. Invariant 8 is no worse than before.
- **Contracts**: nothing under `contracts/` changed, and the refusal strings and codes are unchanged. Only the two refusals the story removes are gone.
- **Guard case**: it does catch the always-hold mutation (M1).
- **Other request types**: a held request that is not `turn/start` (an unknown method) is served in order, either by the main loop or by the next turn's drain.
- **Held request followed by EOF**: behaves like any `turn/start` followed by EOF (the turn ends `failed`, then the connection closes cleanly).
- **Interrupt naming another turn id**: `turnId` is ignored, as it was before the unit (not touched by it).
- **Interleaving interrupt, `turn/start`, interrupt, `turn/start`**: the second `turn/start` is refused by turn 2's drain, the same as a pipelined `turn/start`.

**6. Paths written outside the worktree**, all under `~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1104/adv-u4/`:
- the scratch copy `mut/` (13M of source plus its own build output `mut/target`, 458M)
- the logs `run1.log`, `suite.log`, `fmt.log`, `clippy.log`, `m1.log`, `m2.log`, `base.log`
- the stamp files `run1.start`, `run1.end`, `m1.diffline`, `m2.diffline`, `gate.done`, `mutate.done`
- the scripts `gate.sh`, `mutate.sh`, and `builds.tsv`

No `changelog.md` was written; this pass has no CHANGELOG line.

**Builds and disk**
- `df -h /` before the first build: 83G free.
- Cargo minutes: about 1.5 in total. Wall minutes: about 108, almost all of it waiting for a build slot. Neither the 8G/20-minute tell line nor the 10G/30-minute stop line was reached.
- Peak `target/`: 615M in the tree, 458M in scratch `mut/target`.

`builds.tsv`:
```
2026-11-04T21:44:57Z	0.44	67.42	421M	cargo test -p b10x-harness-app-server --test adversary_1104_u4 (new cases alone)	101
2026-11-04T22:53:03Z	0.66	1	421M (scratch mut/target)	[scratch mut/] m1 hold_or_refuse always holds: cargo test ... --test turn_start_after_interrupt	101
2026-11-04T22:54:43Z	0.13	1	458M (scratch mut/target)	[scratch mut/] m2 deferred push_front: cargo test ... --test turn_start_after_interrupt --test adversary_1104_u4	101
2026-11-04T22:56:21Z	0.03	0	458M (scratch mut/target)	[scratch mut/] base sources 2e37e2e: cargo test ... --test adversary_1104_u4	101
2026-11-04T22:52:41Z	0.13	39	552M	cargo test -p b10x-harness-app-server --no-fail-fast --lib --test turn_start_after_interrupt --test contract --test adversary_d288_u2 --test adversary_1104_u4	101
2026-11-04T23:32:04Z	0.01	0	552M	cargo fmt -p b10x-harness-app-server --check	0
2026-11-04T23:32:04Z	0.12	0	615M	cargo clippy -p b10x-harness-app-server --all-targets --locked -- -D warnings	0
```

**Scope check**

| Path | Status |
|---|---|
| worktree `~/.local/state/worktree/trees/b10x/harness/harness-1104-adv-u4` at 58c2d1a | confirmed |
| unit a158f03, base 2e37e2e, diff confined to `crates/harness-app-server` | confirmed |
| new case file `crates/harness-app-server/tests/adversary_1104_u4.rs` | confirmed |

```findings
- file: crates/harness-app-server/src/session.rs
  line: 140
  category: concurrency
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: settle_interrupts reads the channel and skips deferred, so an interrupt sent after a held request is answered before it and the held turn/start then runs after that interrupt, contradicting fact 3 at session.rs:181
- file: crates/harness-app-server/src/session.rs
  line: 248
  category: concurrency
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: next_frame serves the stash before deferred, so a malformed frame sent after a held turn/start ends the connection with that request unanswered where the base answered it
- file: crates/harness-app-server/src/session.rs
  line: 228
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the unit's three cases pass with deferred drained newest-first because each holds one request; requests_held_behind_an_interrupt_are_served_in_the_order_they_were_sent now catches it
```
