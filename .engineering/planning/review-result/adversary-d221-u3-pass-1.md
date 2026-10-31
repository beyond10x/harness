---
format: aep.planning-md/3
id: review-result:adversary-d221-u3-pass-1
kind: review-result
status: active
title: Adversary pass 1 on the stop union (wave d221 U3)
relations:
- reviews: story:loop-stop-is-a-tagged-union
revision: 1
---
Adversary pass 1 (aep:adversary, opus) on `story:loop-stop-is-a-tagged-union`, unit commit 95bca05, cases committed as e47828d on `adv/d221-u3` (on top of 731d327).

```
unit: story:loop-stop-is-a-tagged-union, commit 95bca05, attacked on adv/d221-u3 at 731d327; cases committed as e47828d
verdict: nothing found
cases: executed 2→15 (stop_union 2, plus adversary_d221_u3 13), red 0
origin: introduced 0 / pre-existing 0 / undecided 0
needs-coordinator: none
```
1. `git diff --stat 731d327 HEAD`: `crates/harness-conformance/tests/adversary_d221_u3.rs | 630 +` only.
2. All 13 cases green: every one of the 12 `LoopStop` variants compiles against the real spec in the `{kind, value}` form (exhaustive match, so a new variant needs a sample); ESS refuses a dropped, unknown, foreign, renamed, wrongly typed or null field, the serde form, an unknown or mis-cased kind, no kind, and `completed` with an empty value; the real `boundary.rs` passes all 29 scenarios and its regressions fail every scenario that sees them; authored scenarios catch a changed value (the generated `harness.loop.Run/outcome/returned` checks no literals); without `--write-projections` a stale `model.json` or `suite.json` is refused with "differs".
3. Runs: `--test adversary_d221_u3`: `test result: ok. 13 passed; 0 failed`; `--test stop_union`: `ok. 2 passed`; `cargo xtask specification`: 29 total 29 passed, inert 29 failed, EXIT=0; fmt and clippy clean.
5. Could not break: field-by-field match with `lib.rs:127-197` (u64/u32 map to ESS `Integer`, which admits negatives; ESS 0.77.0 has no unsigned type); `tagged()` refuses a non-object or kindless stop and passes unknown kinds to ESS, which refuses them; `cargo xtask specification` never passes `--write-projections`; the inert target fails all 29 (`--audit-noop` enforces this for authored scenarios only); `stop_union.rs` reads the real `spec/`.

```findings
[]
```
