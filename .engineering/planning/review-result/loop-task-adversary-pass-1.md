---
format: aep.planning-md/3
id: review-result:loop-task-adversary-pass-1
kind: review-result
status: active
title: 'Adversary pass 1 on LoopTask: red, 5 introduced findings'
relations:
- reviews: story:loop-task-awaits-a-run
revision: 1
---
unit: story:loop-task-awaits-a-run, uncommitted working tree on base 3938235 (branch impl/loop-task-awaits-a-run)
verdict: red
cases: executed 258→264, red 3
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: 2 paths (scratch/mutant/ with a 1.2G target/, scratch/suite.log)
needs-coordinator: decide whether findings 1 and 2 hold the unit (each is a reading of D3/D4, and fixing 2 changes the `run`/`resume_approval` return type); clean up scratch/mutant/target (the global rules forbid me deleting a target/ by hand)

## 1. Diff stat

```
 CHANGELOG.md                               |   5 +
 Cargo.lock                                 |   1 +
 crates/harness-conformance/src/boundary.rs | 312 +++++++++++++++++++++++------
 crates/harness-loop/Cargo.toml             |   3 +
 crates/harness-loop/src/lib.rs             |   2 +
 5 files changed, 261 insertions(+), 62 deletions(-)
```

All five files are the implementor's. My only file is the untracked `crates/harness-loop/tests/adversary_loop_task.rs`, which `git status` lists under `?? crates/harness-loop/tests/`. I changed no implementation file. All mutation was done on a copy in scratch.

## 2. Cases added

All six are in `crates/harness-loop/tests/adversary_loop_task.rs`.

| Case | What it asserts | Now |
|---|---|---|
| `a_call_dropped_before_it_started_performs_no_effect` (:157) | A `call` future dropped while still queued behind another command does not run its tool | red |
| `a_run_that_fails_on_the_wire_hands_back_its_sink` (:209) | When a `run` fails on the wire, the sink and its events come back to the caller, as they do for a synchronous caller; they are not dropped on the task thread | red |
| `a_panic_dropping_an_unread_answer_does_not_kill_the_task` (:278) | A panic while the thread drops an answer nobody will read does not end the thread silently (`shutdown` is not `Gone`) | red |
| `hooks_on_the_ports_are_consulted` (:306) | Hooks attached with `LoopPorts::with_hooks` block a call exactly as in the synchronous loop | green |
| `an_environment_on_the_ports_is_consulted` (:343) | An environment attached with `LoopPorts::with_environment` fails the run closed, as in the synchronous loop | green |
| `the_answer_wakes_the_waker_of_the_last_poll` (:383) | The answer wakes the waker of the most recent poll and not the one it replaced; the executor re-polls only after a wake | green |

The three green cases each catch a mutant the implementor's suite misses (see the mutant run below).

Red output from the first run of this file alone, `cargo test -p b10x-harness-loop --locked --test adversary_loop_task`, before any suite run. The line numbers are from before I applied rustfmt and clippy fixes; a rerun afterwards was the same 3 red / 3 green.

```
---- a_panic_dropping_an_unread_answer_does_not_kill_the_task stdout ----
thread 'harness-loop-task' (150703) panicked at crates/harness-loop/tests/adversary_loop_task.rs:263:13:
the sink's drop exploded
thread 'a_panic_dropping_an_unread_answer_does_not_kill_the_task' (150697) panicked at crates/harness-loop/tests/adversary_loop_task.rs:282:5:
the task's thread died silently: shutdown resolved to Err(Gone)

---- a_call_dropped_before_it_started_performs_no_effect stdout ----
assertion `left == right` failed: a call whose future was dropped before it started still ran its effect
  left: ["first", "abandoned"]
 right: ["first"]

---- a_run_that_fails_on_the_wire_hands_back_its_sink stdout ----
the sink was dropped on the task's thread holding Some(6) events (the synchronous caller kept 6); the caller of LoopTask::run cannot read them

test result: FAILED. 3 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

**Mutant run.** I copied the tree to `scratch/mutant/` and changed only that copy's `task.rs`:
- `agent = agent.with_hooks(hooks);` became `let _ = hooks;` (:83)
- `agent = agent.with_environment(environment);` became `let _ = environment;` (:86)
- `waker.wake();` became `drop(waker);` (:467)

Against that copy:
- The implementor's `awaited.rs` stayed green: `test result: ok. 16 passed; 0 failed; ... finished in 10.01s`.
- The conformance agreement test stayed green: `test result: ok. 1 passed; ... finished in 170.20s`. It was only slower.
- All six of my cases went red. The three that are green against the real code failed with:
  - `the answer arrived without waking the last poll's waker`
  - environment case: `left: Ok(LoopOutcome { stop: Completed, ... }) right: Err(Loop(Environment(Unavailable("no actor view"))))`
  - hooks case: `left: ToolOutcome { output: String("effect"), failed: false, ... } right: ToolOutcome { output: String("`read` was blocked by a hook: ..."), failed: true, ... }`

`cargo clippy -p b10x-harness-loop --test adversary_loop_task --locked -- -D warnings` passes on my file, and it is formatted with rustfmt.

## 3. Suite run, after the cases existed

Command: `cargo test -p b10x-harness-loop -p b10x-harness-conformance --locked --no-fail-fast`, exit status 101.

```
Running unittests src/main.rs (b10x_harness_conformance)   test result: ok. 2 passed; 0 failed
Running unittests src/lib.rs (b10x_harness_loop)           test result: ok. 239 passed; 0 failed
Running tests/adversary_loop_task.rs                       test result: FAILED. 3 passed; 3 failed
Running tests/awaited.rs                                   test result: ok. 16 passed; 0 failed
Doc-tests b10x_harness_loop                                test result: ok. 1 passed; 0 failed
error: 1 target failed: `-p b10x-harness-loop --test adversary_loop_task`
```

The "before" count of 258 is this run minus my file's 6 cases (`adversary_loop_task.rs`). I did not run the suite before my cases existed.

## 4. Findings

All cover the uncommitted working tree on 3938235.

**1. A dropped `call` future still runs its effect.**
- Location: `crates/harness-loop/src/task.rs:323`.
- Verdict: NEEDS-CHANGE, introduced, warning.
- Measured: adversary_loop_task.rs:157 is red. The abandoned `write` call ran after its future was dropped.
- What reaches it: any `tokio::time::timeout` or losing `select!` arm around `task.call(..)` while another command is running. Dropping the future cancels the token, but the synchronous `call` never checks the token. The effect runs and its outcome is thrown away, so a caller that times out and retries writes twice.
- Fix: in the job, skip a command whose future is already gone before it starts. For example, check whether the `Reply` holds the only reference to the slot (`Arc::strong_count == 1`), or set an abandoned flag in `LoopTaskFuture::drop`.

**2. `run` and `resume_approval` lose the sink when the loop returns an error.**
- Location: `crates/harness-loop/src/task.rs:265` (`run`); `resume_approval` at `:306` follows the same code path, which I read but did not test.
- Verdict: NEEDS-CHANGE, introduced, warning.
- Measured: adversary_loop_task.rs:209 is red. Six events were dropped on the task thread. The synchronous caller keeps the same six.
- What reaches it: any wire failure on `run`. D3 says the sink "comes back holding the run's events"; on `Err` there is no sink to return. `run_in` does return it (`RanIn::sink`).
- Fix: return the sink beside the result, for example `(Result<LoopOutcome, LoopError>, S)` or a `RanIn`-like type. That is an API-shape decision for the coordinator.

**3. A panic while dropping an unread answer kills the task thread.**
- Location: `crates/harness-loop/src/task.rs:459` (`fill`; the slot is dropped in `Reply::send`).
- Verdict: INFEASIBLE, introduced, note.
- Measured: adversary_loop_task.rs:278 is red. When the future was already dropped, the answer (including the sink) is dropped on the task thread outside every `catch_unwind`. The thread dies, and `shutdown` resolves to `Gone`. If a port's `Drop` also panics during that unwind, the process aborts.
- What reaches it: nothing found. It needs a sink whose `Drop` panics. I built that state; I know of no such sink.

**4. Hooks and environment attached to `LoopPorts` are untested.**
- Location: `crates/harness-loop/src/task.rs:82`.
- Verdict: CONFIRMED, introduced, warning.
- Measured: removing either attachment leaves `awaited.rs` 16/16 green. My cases at :306 and :343 catch it.
- What reaches it: every embedder that uses `with_hooks` or `with_environment`.

**5. No test detects a lost wake-up.**
- Location: `crates/harness-loop/tests/awaited.rs:608` (`block_on`) and `:31` (`within`), and `crates/harness-conformance/src/boundary.rs:257`.
- Verdict: CONFIRMED, introduced, warning.
- Measured: with `waker.wake()` removed, every test still passes, about 5 s late. `park_timeout` re-polls when it expires, and `tokio::time::timeout` polls the inner future first when its timer fires.
- What reaches it: any executor that relies on the wake. My case at :383 catches it.

## 5. Attacked and not broken

- Dropping a future after its answer arrived: no cancel, because `answer.is_some()` is checked under the lock.
- Lost-wake race between `poll` and `fill`: both happen under one lock, and the waker is woken outside it.
- Polling with a replaced waker: only the last waker is woken (my case :383 is green).
- Re-polling after the future resolved: returns `Gone`, with no cancel on drop.
- Command order with a slow first command, and a `shutdown` queued behind commands in flight.
- A panic in the builder, a port, a hook or the sink's `emit`: all caught.
- A builder `Err`, and a panic in a port's drop at shutdown or after a command panicked.
- Dropping `LoopTask` with commands queued: they still resolve.
- `spawn` and `drop` never block the caller.
- `LoopTask` and its futures are `Send` (the tokio tests compile).
- The agreement test does detect a missing cancel token, config or approver: the cancelled scenario, the budget scenarios and `call-write` respectively.
- A future dropped while queued cancels the token for the command running ahead of it. D4 specifies one token per task, so this is by design, not a finding.

## 6. Paths written outside the worktree

- `wave/loop-task/scratch/mutant/` under the coordinator's scratch root: a source copy of about 10M with the three mutations, plus `target/` at 1.2G.
- `wave/loop-task/scratch/suite.log` under the same root.

## 7. Findings block

```findings
- file: crates/harness-loop/src/task.rs
  line: 323
  category: concurrency
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a call future dropped before its command started still runs the tool effect on the task thread, unreported, so timeout-and-retry duplicates writes
- file: crates/harness-loop/src/task.rs
  line: 265
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: run and resume_approval drop the sink with its events on the task thread when the loop returns LoopError, contrary to D3 and to the synchronous caller who keeps the sink
- file: crates/harness-loop/src/task.rs
  line: 459
  category: concurrency
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: an unread answer is dropped on the task thread outside catch_unwind, so a panicking sink drop kills the thread and shutdown resolves Gone
- file: crates/harness-loop/src/task.rs
  line: 82
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: no implementor test attaches hooks or an environment, so removing either attachment in LoopPorts::agent leaves the suite green
- file: crates/harness-loop/tests/awaited.rs
  line: 608
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: block_on, within and the conformance block_on re-poll on timeout, so removing waker.wake() leaves every test green, only 5 s slower
```
