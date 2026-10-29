---
format: aep.planning-md/3
id: review-result:real-ports-adversary-pass-1
kind: review-result
status: active
title: 'Adversary pass 1 on the real ports under LoopTask: 2 pre-existing findings'
relations:
- reviews: story:real-ports-run-under-an-awaited-loop
revision: 1
---
unit: story:real-ports-run-under-an-awaited-loop, working tree of `impl/real-ports-run-under-an-awaited-loop` at base b87c940 (implementor's changes uncommitted, plus my two new test files)
verdict: CONFIRMED (the code under test is unchanged from base, so the defect predates this unit; it does not hold this unit's acceptance)
cases: executed 413→421, red 2
origin: introduced 0 / pre-existing 2 / undecided 0
wrote-outside-worktree: none on purpose; my test runs made the implementor's helper leave 10 fixture directories in `~/.cache/claude-tmp` (listed in part 6)
needs-coordinator: decide whether the MCP cancellation gap is fixed in this unit (`crates/harness-mcp/src/lib.rs` is the implementor's only if a test proves it necessary, and this case is that test) or becomes its own story

**1. Diff stat**

`git --no-pager diff --stat` shows the implementor's tracked files only:
```
 Cargo.lock                             |  1 +
 crates/harness-cli/Cargo.toml          |  1 +
 crates/harness-cli/tests/end_to_end.rs | 31 ++++---------------------------
 3 files changed, 6 insertions(+), 27 deletions(-)
```
Untracked files I added, both test files:
- `crates/harness-cli/tests/adversary_awaited_ports.rs`
- `crates/harness-cli/tests/fixtures/adversary_silent_mcp.rs`

The fixture is a std-only Rust MCP stdio endpoint. It publishes the controlled fixture's tools byte for byte, so the snapshot digest matches. On `tools/call` it writes a marker file and never answers. Cargo does not treat it as a test target. No non-test path changed.

**2. Cases added** (all in `adversary_awaited_ports.rs`)

| case | what it asserts | now |
|---|---|---|
| `adversary_mcp_shutdown_is_prompt_after_a_dropped_call_{current_thread,multi_thread}` | The MCP call is shown in flight: marker present, run still pending after 200 ms. After the run's future is dropped, `shutdown()` resolves to `Ok(())` within 5 s. | **red** |
| `adversary_llm_shutdown_is_prompt_after_a_dropped_turn_{current_thread,multi_thread}` | The request reached a Responses endpoint that never answers. After the run's future is dropped, `shutdown()` resolves to `Ok(())` within 5 s. | green |
| `adversary_caller_runtime_progresses_during_a_blocked_mcp_call_{current_thread,one_worker}` | While the task thread is stuck in an MCP call, a concurrent tokio task ticks at least 20 times in 1 s. Run on current_thread and on multi_thread with 1 worker. | green |
| `adversary_embedded_shutdown_is_clean_after_runs_dropped_at_any_instant_{current_thread,one_worker}` | 16 tasks over the embedded backend, each run dropped after 0 to 6 ms. Every `shutdown()` resolves to `Ok(())` within 5 s. | green |

Red output, the MCP case run alone before any suite run:
```
running 2 tests
test adversary_mcp_shutdown_is_prompt_after_a_dropped_call_current_thread ... FAILED
test adversary_mcp_shutdown_is_prompt_after_a_dropped_call_multi_thread ... FAILED
thread 'adversary_mcp_shutdown_is_prompt_after_a_dropped_call_current_thread' (1090854) panicked at crates/harness-cli/tests/adversary_awaited_ports.rs:270:5:
assertion `left == right` failed: the task shut down within 5s of the run's future being dropped mid-MCP-call (waited 5.000330391s)
  left: Err(Elapsed(()))
 right: Ok(Ok(()))
thread 'adversary_mcp_shutdown_is_prompt_after_a_dropped_call_multi_thread' (1090855) panicked at crates/harness-cli/tests/adversary_awaited_ports.rs:270:5:
assertion `left == right` failed: the task shut down within 5s of the run's future being dropped mid-MCP-call (waited 5.001213263s)
  left: Err(Elapsed(()))
 right: Ok(Ok(()))
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 6 filtered out; finished in 5.36s
```
To measure the actual wait, I raised the 5 s bound to 60 s in my own file and set it back afterwards. The case then passed in 30.16 s. That is the MCP client's default request timeout (`b10x-mcp-types/src/lib.rs:75`, 30 s), which is also the bound the story's tests use.

**3. Suite run, after my cases existed**

`cargo test -p b10x-harness-cli --locked --no-fail-fast`, exit 101:
- `adversary_awaited_ports`: 6 passed, 2 failed (the two cases above).
- `awaited_ports`: 6 passed. `end_to_end`: 81 passed. Every other binary passed except one.
- `bridge_mode` `the_start_window_is_reached_on_almost_every_attempt` failed once. Run alone 3 times it passed. The machine's load average was 20 to 30, the test counts 18 of 20 timed attempts, and the unit does not touch it. I did not record it as a finding.
- `<before>` = 413 is this same run without my binary's 8 cases.

`awaited_ports` alone, 8 runs in a row: 8 of 8 passed, 0.13 to 0.22 s each.

**4. Findings**

1. **A dropped run does not stop an MCP call.** `crates/harness-mcp/src/lib.rs:188-202`
   - Verdict: CONFIRMED, origin pre-existing.
   - `McpTools::call_within` blocks on `connection.call(&request, remaining)`. The task's token never reaches `McpTools`: `prepare` takes none, and the builder ignores the `&LoopCancel`.
   - So dropping a run's future mid-call (a `timeout` or `select!` in the caller) cancels the token, but the task thread stays blocked until the MCP request timeout of 30 s. `shutdown()` waits that long before resolving `Ok`.
   - `task.rs:186` promises the running command "ends as a cancelled synchronous run does". That holds only after 30 s.
   - Measured: `adversary_awaited_ports.rs:270`, both flavours, exit 101.
   - What reaches it: any async caller that abandons a run during a slow or hung MCP tool. The synchronous CLI's Ctrl-C has the same gap.
   - Origin: `git diff b87c940 -- crates/harness-mcp crates/harness-loop` is empty, so the code is byte-identical at base.
   - Fix to name, not applied: give `McpTools` the task's `LoopCancel` (a constructor that takes it, which the builder passes) and race the call against the token in `call_within`.

2. **The fixture helper leaks a compiled binary per test run.** `crates/harness-cli/tests/common/mod.rs:17`
   - Verdict: CONFIRMED, origin pre-existing. Severity: note.
   - `tempdir().keep()` leaves a 4.6 MB fixture binary in `$TMPDIR` on every run of each test binary that calls it. The unit adds a second caller (`awaited_ports`), which doubles the leak.
   - 130 such directories are already in `~/.cache/claude-tmp`.
   - Origin: the same code is at base in `end_to_end.rs:104-128`.
   - What reaches it: every run of `cargo test -p b10x-harness-cli`.
   - Fix: build into `env!("CARGO_TARGET_TMPDIR")`.

**5. Attacked and not broken**
- **Stub instead of a real port:** a stub cannot pass. Each assertion reads a value only the real fixture produces: the LLM answer text and the request body the endpoint read, "controlled issue is open" from MCP, and the note read back through the catalogue. If the scripted model skips the tool call, `result_of` panics.
- **A port built outside the builder:** `McpTools` or `Embedded` built there would block on or drop a runtime on a runtime thread and panic (reasoned, not run). `LlmPort` built outside and moved in would still pass. It is `Send` and safe on a runtime, so no defect hides there.
- **The LLM call goes over the wire:** yes. The outcome text equals the fixture's literal, and the fixture handed back the request body it read.
- **Dropping a run mid-turn (LLM):** `shutdown()` resolves `Ok` promptly on both flavours.
- **Dropping runs over the embedded backend at 16 different instants:** every `shutdown()` resolved `Ok`; nothing leaked or panicked.
- **Blocked tokio workers:** none. The caller's runtime kept running on current_thread and on multi_thread with 1 worker while the task thread was blocked.
- **`end_to_end`:** the helper moved verbatim (diff checked) and 81 of 81 pass.
- **Timing hidden by the 30 s bound:** none on the happy path, which takes about 0.2 s. The bound only coincides with the MCP timeout in finding 1.

**6. Paths written outside the worktree**
- The scratch dir is unused.
- My test runs made the implementor's helper leave these directories in `~/.cache/claude-tmp/` (about 4.6 MB each; I did not delete them): `.tmpli4QkT`, `.tmpt4HFtj`, `.tmp3uL37g`, `.tmpQOGHjF`, `.tmpFItuRH`, `.tmp4JSHo7`, `.tmpIh2oLI`, `.tmpSZqVxf`, `.tmp9YUS1u`, `.tmpkhySqY`.
- My compiled fixture is inside the worktree, at `target/tmp/adversary-silent-mcp`.

```findings
- file: crates/harness-mcp/src/lib.rs
  line: 202
  category: concurrency
  severity: warning
  verdict: CONFIRMED
  origin: pre-existing
  message: McpTools never sees the LoopTask cancel token, so a run future dropped mid-MCP-call leaves the task thread blocked and shutdown() pending until the 30 s MCP request timeout (adversary_awaited_ports.rs:270 red on both flavours)
- file: crates/harness-cli/tests/common/mod.rs
  line: 17
  category: judgement
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: tempdir().keep() leaks a 4.6 MB fixture binary into TMPDIR on every test-binary run, and the new awaited_ports binary adds a second leak per run
```
