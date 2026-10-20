---
format: aep.planning-md/3
id: review-result:adversary-buildtest-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:build-test-runs-through-the-tools-executor (unit over d69623ea)
relations:
- reviews: story:build-test-runs-through-the-tools-executor
revision: 1
---
build-test U3 adversary pass 1: NEEDS-CHANGE, 7 red cases, 4 warnings, 5 notes. Uncommitted unit over
base d69623ea (tree harness-buildtest-u3, branch impl/build-test-runs-through-the-tools-executor).

Added `crates/harness-catalogue/tests/toolchain_adversary.rs` (10 cases) and 2 cases appended to
`crates/harness-substrate/src/tests.rs` (after line 1509): 5 green, 7 red.

Findings:

1. `crates/harness-catalogue/src/catalogue.rs:566` (warning, introduced) — a plan that starts no
   process now refuses ("toolchain provider produced no output") and a role drops that provider from
   `toolchains`, where 0.15.1 answered `[]` and kept the key; not in the CHANGELOG.
2. `crates/harness-catalogue/src/catalogue.rs:567` (warning, introduced) — a role cut short by the
   shared deadline omits the providers that never started and says nothing, so a cut-short answer
   reads as complete (invariant 8).
3. `crates/harness-catalogue/src/catalogue.rs:559` (warning, pre-existing) — a `writes:` parameter left
   to its spec default is never put to the write scope (`plan.writes` reads only explicit arguments;
   same in tools 0.3.0 `lib.rs:703` and in harness's old `invoke_toolchain`). Reach: an
   operator-loaded `--toolchain-spec` with a defaulted write path. No built-in has one.
4. `crates/harness-catalogue/src/scope.rs:164` (warning, pre-existing) — a written path naming a
   directory is not matched by a deny rule over that directory's contents, so a built-in formatting
   tool given a directory rewrites files the scope refuses, locally and under Split plus confined.
5. `crates/harness-catalogue/src/catalogue.rs:562` (note) — the step-scope half of `refusing_writes`
   had no test; now pinned (green).
6. `crates/harness-catalogue/src/catalogue.rs:594` (note, pre-existing) — the `run` refusal of
   toolchain programs had no test; now pinned (green).
7. `crates/harness-catalogue/src/toolchain.rs:5` (note) — module doc says `invoke_toolchain` moved to
   tools; `Catalogue::invoke_toolchain` still exists.
8. `CHANGELOG.md:155` (note) — a second line names the dash-prefix trigger beside the one-line
   Security entry.
9. `crates/harness-substrate/tests/conformance.rs:1663` (note, INFEASIBLE) — `cargo xtask gate` never
   sets `B10X_CGROUP_ROOT`, so the gate runs only the non-confining branch; the confined acceptance
   was met in a manual delegated run (passed).

Attacked and could not break: spelling, `..` and absolute paths; a link into a denied directory
(local and confined); a refused write in a later group starts nothing; `Some(ZERO)` starts nothing;
confined calls use the same `run_within`, environment and root as 0.15.1; reply shapes of built-ins
other than `rust_fmt`; CheckFailed wording; the dash refusal through `executor::plan`; invariants
2, 6, 13; no leftover references to deleted symbols.

```findings
[{"file":"crates/harness-catalogue/src/catalogue.rs","line":566,"category":"contract-drift","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"A toolchain plan that starts no process now refuses (and a role drops that provider) where 0.15.1 answered [] and kept the key; not in the CHANGELOG."},
{"file":"crates/harness-catalogue/src/catalogue.rs","line":567,"category":"acceptance","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"A role cut short by the shared deadline omits the providers that never started, so a cut-short answer reads as complete (invariant 8)."},
{"file":"crates/harness-catalogue/src/catalogue.rs","line":559,"category":"boundary","severity":"warning","verdict":"CONFIRMED","origin":"pre-existing","message":"A writes: parameter left to its spec default is never put to the write scope; plan.writes reads only explicit arguments (tools lib.rs:703)."},
{"file":"crates/harness-catalogue/src/scope.rs","line":164,"category":"boundary","severity":"warning","verdict":"CONFIRMED","origin":"pre-existing","message":"A written path naming a directory is not matched by a deny rule over its contents, so a built-in formatting tool given a directory rewrites files the scope refuses."},
{"file":"crates/harness-catalogue/src/catalogue.rs","line":562,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"The step-scope half of refusing_writes had no test; now pinned."},
{"file":"crates/harness-catalogue/src/catalogue.rs","line":594,"category":"mutant","severity":"note","verdict":"CONFIRMED","origin":"pre-existing","message":"The run refusal of toolchain programs had no test; now pinned."},
{"file":"crates/harness-catalogue/src/toolchain.rs","line":5,"category":"contract-drift","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"Module doc says invoke_toolchain moved to tools while Catalogue::invoke_toolchain still exists."},
{"file":"CHANGELOG.md","line":155,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"A second committed line names the dash-prefix trigger beside the one-line Security entry."},
{"file":"crates/harness-substrate/tests/conformance.rs","line":1663,"category":"judgement","severity":"note","verdict":"INFEASIBLE","origin":"introduced","message":"cargo xtask gate never sets B10X_CGROUP_ROOT, so the confined acceptance is met only by a manual delegated run."}]
```
