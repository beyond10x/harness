---
format: aep.planning-md/3
id: review-result:adversary-d288-u4-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave d288 U4 (loop modelled in ESS)
relations:
- reviews: story:harness-loop-modeled-in-ess
revision: 1
---
Adversary pass 1 on U4 (`story:harness-loop-modeled-in-ess`), wave d288. It reviews unit `6c7a720`
as merged at `a09dc2c`; its cases are `cee5c4d` on `adv/d288-u4`. The adversary's report, as
returned, condensed where it repeated logs:

unit: U4 adversary pass 1
verdict: NEEDS-CHANGE
cases: executed 16→26, red 1
origin: introduced 5, pre-existing 2, undecided 0
wrote-outside-worktree: 11 files and 3 directories under the assigned scratch directory adv-u4/ (mutants, proposed scenarios, a validate copy)
needs-coordinator: two questions. Should the three transitions the loop takes in a two-call turn be declared or listed as UNMAPPED? Should the three proposed scenarios be adopted?

Cases (`crates/harness-conformance/tests/adversary_d288_u4.rs`):
- Nine are green on the real code. Each kills a mutant that the ESS suite passes: M01, M02, M03, M04, M05, M06, M07, M08, M09, M13, M14 and M15.
- `every_step_of_turns_with_two_calls_is_a_declared_transition` is red. Three steps the real loop takes are not declared in `spec/domains/loop.yaml`:
  - `CompleteTool AwaitingTool -> AwaitingApproval`
  - `Deny AwaitingApproval -> AwaitingTool`
  - `CompleteTool AwaitingTool -> AwaitingTool`

Mutants of `crates/harness-loop/src/lib.rs` (18 in all):
- The ESS suite kills M10, M11, M12, M13, M14, M16, M17 and M18.
- M01 to M09 and M15 survive it with 165/165.

Suite: 16 cases passed before. After: 1 + 13 + 9 + 2 passed and 1 failed, exit 101. fmt and clippy are clean, and `cargo xtask specification` gives 165/165.

```findings
- file: spec/domains/loop.yaml
  line: 188
  category: acceptance
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a two-call turn takes CompleteTool AwaitingTool->AwaitingApproval, Deny AwaitingApproval->AwaitingTool and CompleteTool AwaitingTool->AwaitingTool (run_calls lib.rs:3363-3468), which the lifecycle neither declares nor lists as UNMAPPED
- file: spec/COVERAGE.md
  line: 30
  category: mutant
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the 80% trigger claim is unmeasured because the adapter's long text fills 100% of the window, so M01 (lib.rs:1105) and M02 (lib.rs:3122) pass 165/165
- file: spec/domains/loop.yaml
  line: 92
  category: mutant
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: after_compaction is only ever asserted false, so M03 (lib.rs:2747) and M04 (lib.rs:3147) pass 165/165
- file: spec/COVERAGE.md
  line: 14
  category: mutant
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the checkpoint claim survives M05 checkpoint id (2456), M06 remaining_calls (2460), M08 no re-deferral on resume (2398) and M15 deferred call answered twice (3460-3466)
- file: spec/COVERAGE.md
  line: 12
  category: judgement
  severity: note
  verdict: NEEDS-CHANGE
  origin: introduced
  message: 83 of 165 cases assert out-of-state and unknown-run refusals that the adapter's awaited() table (boundary.rs:687) computes, not AgentLoop
- file: spec/COVERAGE.md
  line: 81
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: pre-existing
  message: M07 checkpoint deadline (lib.rs:2462) and M09 ledger cost (lib.rs:1030) survive ESS inside the disclosed UNMAPPED cost and deadline gap
- file: spec/domains/loop.yaml
  line: 267
  category: contract-drift
  severity: note
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: the Call command cites lib.rs:2073, a closing brace; AgentLoop::call is at lib.rs:2259
```

## Coordinator routing

All findings except F6 go to U4 correction 1 in this wave.

- **F1:** the three transitions are real loop behaviour, so declare them in `loop.yaml`, citing `run_calls`. Listing them as UNMAPPED would leave a model of the loop that the loop contradicts.
- **F2:** add a scenario that puts the trigger inside the window (below 80% does not compact, above it does), if the adapter can size its text. If it cannot, narrow the COVERAGE claim to what is measured and name the Rust case that holds it.
- **F3 and F4:** adopt the three proposed scenarios. COVERAGE names the Rust adversary cases that hold the checkpoint id, the remaining calls and answered-once (M05, M06 and M15), and claims nothing wider than the scenarios and those cases.
- **F5:** COVERAGE says the out-of-state and unknown-run refusals are the adapter's view of the loop, computed by `awaited()`, and not refusals made by `AgentLoop`.
- **F6:** no change. The disclosure is accurate, and the adversary's Rust cases now hold both.
- **F7:** fix the citation, as an in-scope fix to a file this unit owns.
