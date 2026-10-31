---
format: aep.planning-md/3
id: review-result:loop-task-adversary-pass-2
kind: review-result
status: active
title: 'Adversary pass 2 on LoopTask: 1 introduced finding'
relations:
- reviews: story:loop-task-awaits-a-run
revision: 1
---
unit: story:loop-task-awaits-a-run, pass 2, uncommitted working tree on 3938235
verdict: NEEDS-CHANGE
cases: executed 267→272, red 1
origin: introduced 1 / pre-existing 0 / undecided 0
wrote-outside-worktree: 4 paths (`scratch/mutant/` reused, `scratch/task.rs.orig`, `scratch/mutants.sh`, `scratch/suite-pass2.log`)
needs-coordinator: `scratch/mutant/target` is now about 1.2G; I may not delete a target/ by hand, so cleaning it up is yours

The fixes for my five pass-1 findings hold. One new defect breaks the fix for finding 3: if a port panics during a run whose future was dropped, that panic is lost. `shutdown` resolves `Ok(())` instead of `Panicked`.

## 1. Diff stat

```
 CHANGELOG.md                               |   6 +
 Cargo.lock                                 |   1 +
 crates/harness-conformance/src/boundary.rs | 312 +++++++++++++++++++++++------
 crates/harness-loop/Cargo.toml             |   3 +
 crates/harness-loop/src/lib.rs             |   2 +
```

These are all the implementor's files. My only addition is the untracked `crates/harness-loop/tests/adversary_loop_task_pass2.rs`. I changed no implementation file, and mutated only the scratch copy, restoring it after each mutant.

**The implementor's three edits to `adversary_loop_task.rs` (:245, :364, :429) weaken nothing.** Each one only follows the switch to `Ran`:
- :245 now requires `Ok(ran)` with `ran.result.is_err()`, which is stricter than before.
- :364 still compares the result for exact equality.
- :429 still asserts the text `"done"`.

One side effect: my pass-1 panic test now reaches the skipped-command path, not the unread-answer path. The pass-2 case at :306 covers the unread-answer path.

## 2. Cases added

All five are in `crates/harness-loop/tests/adversary_loop_task_pass2.rs`.

| Case | What it asserts | Now |
|---|---|---|
| `a_panic_in_a_run_whose_future_was_dropped_is_reported_at_shutdown` (:210) | A port that panics after its run's future was dropped is reported by `shutdown` as `Panicked` | **red** |
| `shutdown_reports_the_first_unreported_panic` (:239) | Two skipped sinks panic, then a port's drop panics; `shutdown` reports the first panic | green |
| `after_a_panicking_port_the_ports_are_gone_and_later_commands_answer_the_panic` (:268) | After a panic the ports are dropped; a later `call` answers the same `Panicked` and never reaches the tool | green |
| `a_panicking_sink_of_an_unread_answer_is_reported_at_shutdown` (:306) | The future is dropped after its command started and its unread sink panics on drop; `shutdown` reports it | green |
| `a_future_dropped_at_its_start_either_skips_or_cancels` (:330) | Over 200 rounds, dropping a future right after sending it never cancels the token for a command that did not run, and never hangs | green |

Red output from running this file alone, before any suite run:

```
thread 'harness-loop-task' (584395) panicked at crates/harness-loop/tests/adversary_loop_task_pass2.rs:218:17:
the model port panicked on cancel
thread 'a_panic_in_a_run_whose_future_was_dropped_is_reported_at_shutdown' (584390) panicked at crates/harness-loop/tests/adversary_loop_task_pass2.rs:228:5:
the port's panic reached nobody: shutdown resolved to Ok(())
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
```

**Mutant run.** Each mutant was applied alone to the scratch copy (`scratch/mutants.sh`). Under every one, `awaited.rs` stayed `ok. 19 passed` and my pass-1 file stayed `ok. 6 passed`, so neither notices them:

| Mutant | What it changes | Caught only by |
|---|---|---|
| K1 | `keep` keeps the last unreported panic (`unreported = Some(error)`) | `shutdown_reports_the_first_unreported_panic` |
| K2 | The ports are not retired after a command panic (`task.rs:585-587` deleted) | `after_a_panicking_port_…` (`the ports outlived the panic`) |
| K3 | `finish` prefers the port drop's panic over an earlier one | `shutdown_reports_the_first_unreported_panic` |

`cargo clippy -p b10x-harness-loop --test adversary_loop_task_pass2 --locked -- -D warnings` is clean, and the file is formatted with rustfmt.

## 3. Suite run, after the cases existed

Command: `cargo test -p b10x-harness-loop -p b10x-harness-conformance --locked --no-fail-fast`, exit status 101.

```
unittests src/main.rs (conformance)    ok. 2 passed
unittests src/lib.rs (harness_loop)    ok. 239 passed
tests/adversary_loop_task.rs           ok. 6 passed
tests/adversary_loop_task_pass2.rs     FAILED. 4 passed; 1 failed
tests/awaited.rs                       ok. 19 passed
Doc-tests b10x_harness_loop            ok. 1 passed
error: 1 target failed: `-p b10x-harness-loop --test adversary_loop_task_pass2`
```

The "before" count of 267 is this run with `adversary_loop_task_pass2.rs` (5 cases) left out.

## 4. Finding

**A panic in a command whose future was dropped mid-run is lost.**
- Location: `crates/harness-loop/src/task.rs:381`; the drop itself happens in `fill` at :527.
- Verdict: NEEDS-CHANGE, introduced, warning.
- Measured: adversary_loop_task_pass2.rs:210 is red.
- How it happens:
  1. `execute` catches the panic, retires the ports, and returns `Err(Panicked)`.
  2. Because the future is gone, `fill` drops that answer unread.
  3. Nothing calls `keep`, so `shutdown` resolves `Ok(())`.
- This contradicts the type's own documentation (task.rs:197-199): "A panic nobody is waiting to hear about … is caught too and kept: … `shutdown` resolves to it."
- What reaches it: a run wrapped in `timeout` or `select!` whose future is dropped, plus a port that panics when the token is cancelled (for example, one that unwraps a cancelled read). Only a later command on the same task would ever see the panic.
- Fix: in the job, when the slot is abandoned and the answer is `Err(Panicked)`, pass it to `station.keep`. For example, have `fill` return the unread answer and let the job inspect it.

## 5. Attacked and not broken

- **Dropping a future exactly at start:** start and drop decide under one lock, so the command is either skipped or running. The 200-round stress case passed.
- **Dropping after start but before the answer:** the token is cancelled and the answer is dropped on the task thread, inside the catch.
- **Polling twice:** a second poll after `Ready` returns `Gone` without cancelling.
- **Skipped commands:** each one answers its own slot with `Gone` and is dropped inside the job. No other waiter exists and nothing leaks.
- **A future dropped while queued:** it does not cancel the token, and later runs complete. Both are tested in `awaited.rs` and confirmed by the stress case.
- **Panic reporting at `shutdown`:** the first panic is kept, and a panic already reported to a live future is not reported again.
- **`Ran` against the synchronous calls:** it matches synchronous `run` and `resume_approval` on success and on wire error (`awaited.rs:586` and my pass-1 cases). The cancelled run goes through the same `agent.run` path.
- **Docs and CHANGELOG:** `LoopTaskError::Loop` has no remaining users, and the CHANGELOG now lists `Ran`.

## 6. Paths written outside the worktree

All under `wave/loop-task/scratch/` in the coordinator's scratch root:
- `mutant/`: reused and resynced to the corrected source; its `target/` is about 1.2G.
- `task.rs.orig`
- `mutants.sh`
- `suite-pass2.log`
- `suite.log` from pass 1 is still there.

## 7. Findings block

```findings
- file: crates/harness-loop/src/task.rs
  line: 381
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a Panicked answer for a command whose future was dropped mid-run is discarded by fill without keep, so shutdown resolves Ok(()) and the documented "kept, shutdown resolves to it" does not hold
```
