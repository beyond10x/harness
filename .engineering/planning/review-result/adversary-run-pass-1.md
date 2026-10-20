---
format: aep.planning-md/3
id: review-result:adversary-run-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:run-moves-onto-tool-process (unit over df37462b)
relations:
- reviews: story:run-moves-onto-tool-process
revision: 1
---
run U2 adversary pass 1: NEEDS-CHANGE, 1 red case, 1 blocker, 1 warning, 2 notes. Uncommitted unit over
base df37462b (tree harness-run-u2, branch impl/run-moves-onto-tool-process).

Added `crates/harness-catalogue/tests/run_adversary.rs`, 11 cases (a verbatim copy of 0.15.1's runner as
the comparison): 10 green, 1 red. Green: replies byte-for-byte equal to 0.15.1's for lengths 0 to
200001 (65536 and 65537 included), 2/3/4-byte characters at every cut offset, 12 seeded random streams,
exit codes and signals; refusal words and the declared set; the environment name for name with and
without `inheriting`; a symlinked root; a spent or short bound; a double-forked and a stderr-only
descendant bounded; `rust_fmt_check` through `LocalOperations` equal to 0.15.1.

Findings:

1. `crates/harness-catalogue/src/local.rs:292` (blocker) — `run_within`'s rustdoc links the private fn
   `refused`, so the gate's strict rustdoc step exits 101.
2. `crates/harness-catalogue/src/local.rs:301` (warning, INFEASIBLE) — `b10x-tool-process` sends
   `killpg` at the bound after the program was reaped; when the group is empty its id may already
   belong to another group. Red case `the_kill_at_the_bound_does_not_reach_a_group_that_took_the_programs_freed_id`
   (built with `ns_last_pid`); 0.15.1's runner never signalled after reaping (control case green). In
   production it needs a pid wrap within the bound (`pid_max` 4194304 here). The fix is upstream
   (`waitid` with `WNOWAIT`).
3. `CHANGELOG.md:93` (note) — "not valid UTF-8 near a cut" understates it: one invalid byte 32 KiB
   from the cut changes `omitted_bytes` (4465 against 4464).
4. `crates/harness-cli/src/workflow.rs:1545` (note) — rustdoc still names `local.rs` as the writer of
   the local reply's `exit`; it is `tool_process::RunOutput` now.

Suite: workspace `--no-fail-fast` 994 passed, 1 failed (the red case), 1 ignored; substrate conformance
35 passed; clippy exit 0. No temp directories or processes left.

```findings
[{"file":"crates/harness-catalogue/src/local.rs","line":292,"category":"acceptance","severity":"blocker","verdict":"NEEDS-CHANGE","origin":"introduced","message":"run_within's rustdoc links the private fn refused, so the gate's strict rustdoc step exits 101 and cargo xtask gate cannot exit 0"},
{"file":"crates/harness-catalogue/src/local.rs","line":301,"category":"concurrency","severity":"warning","verdict":"INFEASIBLE","origin":"introduced","message":"process.run kills the program's group id at the bound after the program was reaped, so a group that took the freed id is SIGKILLed (shown with a constructed pid reuse; 0.15.1 never did this)"},
{"file":"CHANGELOG.md","line":93,"category":"contract-drift","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"'not valid UTF-8 near a cut' understates the change; an invalid byte 32 KiB from the cut changes omitted_bytes (4465 vs 4464)"},
{"file":"crates/harness-cli/src/workflow.rs","line":1545,"category":"contract-drift","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"the rustdoc still names harness-catalogue/src/local.rs as the writer of the local reply's exit, now tool_process::RunOutput"}]
```
