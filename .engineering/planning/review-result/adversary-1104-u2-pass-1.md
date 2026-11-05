---
format: aep.planning-md/3
id: review-result:adversary-1104-u2-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave 2026-11-04 U2 (MCP call stops when the loop is cancelled)
relations:
- reviews: story:mcp-call-stops-when-the-loop-is-cancelled
revision: 1
---
```
unit: U2 story:mcp-call-stops-when-the-loop-is-cancelled (unit c6030cb); findings cover tree review/adv-u2 at 3ebe63f plus the uncommitted crates/harness-cli/tests/adversary_1104_u2.rs
verdict: green
cases: executed 18→22, red 0
origin: introduced 2, pre-existing 0, undecided 1
wrote-outside-worktree: 1 directory (the assigned scratch), see part 6
needs-coordinator: no
```

The unit's code holds: I found no defect in it. All three findings are mutants that the unit's suite lets through. The four new cases are green on the tree and each one fails on the mutant it was written for. Your gate is green with them in place.

**1. Diff.** `git diff --stat` is empty because the only change is a new untracked file. `git status --short` shows:
```
?? crates/harness-cli/tests/adversary_1104_u2.rs
```
That is one test file of 370 lines. No implementation file was touched.

**2. Cases added** (`~/.local/state/worktree/trees/b10x/harness/harness-1104-adv-u2/crates/harness-cli/tests/adversary_1104_u2.rs`). On the tree all four are green: `test result: ok. 4 passed; 0 failed` on the first run, made before any suite run. They go red only on the mutants:

| Case | What it asserts | Red on | Red output |
|---|---|---|---|
| `adversary_1104_u2_a_call_after_the_cancel_is_never_sent` | With the token already cancelled: a failed outcome, no refusal, the exact "not sent" text, and the server never sees the call | M2 | `left: "MCP call `read_issue` was cancelled with the run before it answered; the server was not told and may still complete it"` / `right: "MCP call `read_issue` was not sent: the run was cancelled"` |
| `..._a_call_cancelled_in_flight_is_a_failed_outcome_saying_it_was_sent` | Cancelled mid-call: a failed outcome within 5 s, no refusal, the exact "server was not told" text | M4 | `the call returned 29.991660869s after the cancel, not within 5s; story:mcp-call-stops-when-the-loop-is-cancelled` |
| `..._the_remaining_time_still_bounds_a_call_the_token_never_cancels` | `call_within(.., Some(500ms))` with a token that never fires ends within 5 s as `MCP call failed:`, not as a cancel | M3 | `a call with 500 ms remaining returned after 30.000977796s; story:mcp-call-stops-when-the-loop-is-cancelled` |
| `..._ctrl_c_ends_a_cli_run_waiting_on_an_mcp_call` | The real binary with `--mcp-profile` pointed at the silent stdio server: after SIGINT mid-call it exits within 5 s, and its record has `tool-completed` for `call_b10x_001` with `failed: true` | M1, M4 | `5.016138355s after SIGINT the run was still waiting on its MCP call, not ended within 5s; story:mcp-call-stops-when-the-loop-is-cancelled` |

**Mutations.** All ran in the scratch copy `<scratch>/mut`, never in the tree:

| Mutant | Change | Unit's suite (mcp lib 3, awaited_ports 6, adversary_awaited_ports 9) | New cases |
|---|---|---|---|
| M1 | `prepare` passes `&LoopCancel::new()` to `with_mcp` (cli lib.rs:2341) | **survives**: 9 passed, 6 passed | Ctrl-C case red |
| M2 | pre-send check removed (`if false && ...`, mcp lib.rs:219) | **survives**: 3, 6, 9 passed | never-sent case red |
| M3 | `connection.call(&request, None)` instead of `remaining` (mcp lib.rs:233) | **survives**: 3, 6, 9 passed | remaining-time case red |
| M4 | `until_cancelled` never resolves | **killed**: unit test fails; both `adversary_mcp_shutdown_is_prompt_*` fail (`waited 5.002036071s`); the synchronous case fails (`29.990937008s`) | in-flight and Ctrl-C cases red |

M4 answers your question: the unit's rewritten cases and its new synchronous case do fail when the token is ignored. But none of the unit's cases goes through `with_mcp`, so M1 survives.

**3. Suite in the tree**, run after the new file existed. Every call exited 0:
- `cargo test -p b10x-harness-mcp --lib`: `test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s`
- `--test awaited_ports`: `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s`
- `--test adversary_awaited_ports`: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.24s`
- `--test adversary_1104_u2`: `test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s`
- fmt on mcp and on cli: exit 0. Clippy `-D warnings` on mcp and on cli: exit 0. Cli clippy is not in your gate list; the common rules ask for it because I changed that crate.

**4. Findings** (tree 3ebe63f):

| file:line | What was measured | What reaches it | Verdict | Origin |
|---|---|---|---|---|
| crates/harness-cli/src/lib.rs:2341 | M1 leaves the unit's suite green; only the new Ctrl-C case catches it | `run` and `chat` through `prepare`; Ctrl-C through `install_interrupt` | CONFIRMED | introduced |
| crates/harness-mcp/src/lib.rs:219 | M2 leaves the suite green; the unit test only checks `cancelled_outcome(..).failed` | the loop checks the token before every call (harness-loop lib.rs:3489), so this line is reached only when a cancel lands between that check and the call, for example during a before-call hook | CONFIRMED | introduced |
| crates/harness-mcp/src/lib.rs:233 | M3 leaves the suite green: no test passes a deadline to `McpTools::call_within` | the loop's `max_duration_ms` deadline, passed on by `Augmented::call_within` (cli lib.rs:3343) | CONFIRMED | undecided |

For M3, the base line is the same and no base test calls `call_within` (checked with `git grep`), but I did not run it against the base, so the origin stays undecided. The fix for all three is to commit the new file.

**5. Attacked and could not break:**
- **Connection reuse after an abandoned call:** cancel cannot reach it. A task's token stays cancelled (task.rs:215) and the CLI never resets its token. rmcp 3.2.0 drops a late answer (`let _ = responder.send(response)`, service.rs:1461) and its request ids only count up.
- **`shutdown()` after a dropped run:** green, and M4 shows the cases guarding it can fail.
- **`biased` order:** a cancel beats an answer that is ready at the same moment. The text only says the server "may still complete it", so this is not wrong. I could not test the race deterministically.
- **Who holds the token:** `run` and `chat` get the run's token. The tools listing never calls a tool. The app-server builds no MCP tools. Delegates share the parent's tools and token.
- **Failed outcome, not a refusal:** cases A, B and D assert it, and the model still gets an answer for its call id.

**builds.tsv** (UTC start, cargo min, wall min, target size, command, exit; `m*` and `clean-mut` rows are the scratch copy's target, the rest the tree's):
```
2026-11-05T00:44:05Z	0.88	1.38	2.0G	t1-new: cargo test -p b10x-harness-cli --test adversary_1104_u2	0
2026-11-05T00:45:38Z	1.12	3.63	2.3G	m1: cargo test -p b10x-harness-cli --test adversary_1104_u2 --test awaited_ports --test adversary_awaited_ports --no-fail-fast	101
2026-11-05T00:49:33Z	0.72	11.23	2.5G	m23: cargo test -p b10x-harness-mcp --lib -p b10x-harness-cli --test adversary_1104_u2 --test awaited_ports --test adversary_awaited_ports --no-fail-fast	101
2026-11-05T01:01:16Z	0.00	0.83	2.5G	m4: cargo test -p b10x-harness-mcp --lib --test adversary_awaited_ports --test adversary_1104_u2 --no-fail-fast	101
2026-11-05T01:02:16Z	0.42	4.08	3.0G	m4-mcp: cargo test -p b10x-harness-mcp --lib	101
2026-11-05T01:06:21Z	1.10	1.10	3.0G	m4-cli: cargo test -p b10x-harness-cli --test adversary_awaited_ports --test adversary_1104_u2 --no-fail-fast	101
2026-11-05T01:07:48Z	0.25	5.58	2.5G	g1-mcp-lib: cargo test -p b10x-harness-mcp --lib	0
2026-11-05T01:13:23Z	0.05	0.05	2.7G	g2-awaited: cargo test -p b10x-harness-cli --test awaited_ports	0
2026-11-05T01:13:26Z	0.03	0.03	2.8G	g3-adv-awaited: cargo test -p b10x-harness-cli --test adversary_awaited_ports	0
2026-11-05T01:13:28Z	0.02	0.02	2.8G	g4-adv-1104-u2: cargo test -p b10x-harness-cli --test adversary_1104_u2	0
2026-11-05T01:13:29Z	0.00	0.00	2.8G	g5-fmt-mcp: cargo fmt -p b10x-harness-mcp --check	0
2026-11-05T01:13:29Z	0.02	0.02	2.8G	g6-fmt-cli: cargo fmt -p b10x-harness-cli --check	0
2026-11-05T01:13:30Z	0.18	0.18	2.9G	g7-clippy-mcp: cargo clippy -p b10x-harness-mcp --all-targets --locked -- -D warnings	0
2026-11-05T01:13:41Z	0.32	0.32	3.4G	g8-clippy-cli: cargo clippy -p b10x-harness-cli --all-targets --locked -- -D warnings	0
2026-11-05T01:14:11Z	0.00	1.83		clean-mut: cargo clean --manifest-path <scratch>/mut/Cargo.toml	0
```
- **Totals:** 5.11 cargo minutes and 30.28 wall minutes. Neither the 8G/20-minute tell line nor the stop line was reached.
- **Disk:** `df -h /` showed 79G free before the first build.
- **Peak target:** 3.4G in the tree. The scratch copy peaked at 3.0G and has been cleaned.

**Deviations:**
- Each cargo call ran inside the slot through `<scratch>/timed.sh`, to separate cargo time from the slot wait. Otherwise the call shape is the one in the common file.
- In the `m23` run, `--lib` applied to both packages, so it also ran harness-cli's 295 lib tests, in the scratch copy.
- The first `m4` call named a target in the wrong package and failed before compiling anything (0 cargo minutes).

**Scope:** every path the brief named is confirmed: `crates/harness-mcp/src/lib.rs` (`call_within`, `until_cancelled`), `crates/harness-cli/src/lib.rs` (`with_mcp`, `prepare`, `tools_command`), `crates/harness-cli/tests/adversary_awaited_ports.rs`, and the tree on branch review/adv-u2 at 3ebe63f.

**6. Written outside the worktree:**
- The assigned scratch directory `~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1104/adv-u2/`, holding: `build.sh`, `timed.sh`, `gate.sh`, `builds.tsv`, the `log-*.txt` and `stamp-*.txt` files, `mcp-lib.rs.{orig,m23,m4}`, `m1-lib.rs.mutated`, and `mut/` (a 14M source copy; its target is cleaned; harness-mcp is left in its M4 state).
- The harness task-output files under `~/.cache/claude-tmp`.
- My own session lease, `adv-u2-1104-pass1`, taken and released through `worktree hook`.

```findings
- file: crates/harness-cli/src/lib.rs
  line: 2341
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: handing with_mcp a fresh token in prepare leaves every unit case green (adversary_awaited_ports 9, awaited_ports 6), so the CLI Ctrl-C path the story names had no test; adversary_1104_u2_ctrl_c_ends_a_cli_run_waiting_on_an_mcp_call kills it (5.016 s after SIGINT).
- file: crates/harness-mcp/src/lib.rs
  line: 219
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: dropping the pre-send check survives the suite because the unit test only asserts cancelled_outcome(..).failed, and the model would then be told a never-sent call was sent; adversary_1104_u2_a_call_after_the_cancel_is_never_sent pins the text and that the server never sees the call.
- file: crates/harness-mcp/src/lib.rs
  line: 233
  category: mutant
  severity: warning
  verdict: CONFIRMED
  origin: undecided
  message: replacing remaining with None in the raced call survives the suite (no test passes a deadline to McpTools::call_within), so a run deadline could wait the full 30 s; adversary_1104_u2_the_remaining_time_still_bounds_a_call_the_token_never_cancels kills it (30.0 s).
```
