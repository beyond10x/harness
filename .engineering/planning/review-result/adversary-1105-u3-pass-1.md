---
format: aep.planning-md/3
id: review-result:adversary-1105-u3-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave 2026-11-05 U3 (tools names its write scope)
relations:
- reviews: story:tools-help-matches-its-output
revision: 1
---
```
unit: U3, `story:tools-help-matches-its-output`, tree harness-1105-adv-u3 at eb83d8c (unit 10299fb)
verdict: red
cases: executed 32→34, red 2
origin: introduced 1, pre-existing 0, undecided 0
wrote-outside-worktree: `~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1105/adv-u3/` (builds.tsv, the *.log, *.start and *.end files, lib.rs.orig and lib.rs.m1 to m3, `mut/`)
needs-coordinator: yes. The fix is an implementation change, which an adversary does not make.
```

**1. Diff.** `git --no-pager diff --stat` is empty because the only change is a new, untracked file. `git status --short` gives `?? crates/harness-cli/tests/adversary_1105_u3.rs`, 363 lines, a test file. No implementation file was touched.

**Finding**

| file:line | what was measured | what reaches it | verdict | origin |
|---|---|---|---|---|
| `crates/harness-cli/src/lib.rs:4263` | `tools_command` passes `None` as the configured scope. `run` uses the operator's configured scope and `tools` prints a different one. On one machine, `run --substrate-embedded` with `[default] write-scope = ["**=allowed"]` rewrote `.b10x/harness.toml`. Then `tools --substrate-embedded` printed `.b10x/**` as `denied` (`adversary_1105_u3.rs:319`). With `[default] write = true`, `run` with no flags wrote `notes.md` under the default scope, and `tools` with no flags printed `write_scope: []`, which the code comment says means "this run writes nowhere" (`:351`). | The operator's own `[default]` table, which applies to every run without `-p`. The profiles guide says a profile `write-scope` replaces the default. The confinement guide says to use `tools` "with the same confinement flags you plan to give `run`". Since `tools` takes no `-p`, `[default]` is the only profile input both commands get under the same flags. Existing test `a_profile_write_scope_applies_to_a_typed_embedded_run` shows `run` applying it. | CONFIRMED | introduced |

On the origin: that `tools` ignores profiles is older than the unit. At ed73241 its catalogue already ignored `write = true`. But at the base the document had no `write_scope` key and claimed no scope. The false claim (`.b10x/**` denied while the run rewrites it) comes from the unit's new key. That is also the file the default scope exists to protect.

Two possible fixes, not applied:
- `tools_command` resolves `[default]` the way `profiles explain` does (`explain_writing`, lib.rs:4044). It would pass `wanted.write_scope` as `configured` and treat `wanted.write == Some(true)` as `writes`.
- Or the key and the `--write-scope` help (lib.rs:1210) say they describe the typed flags only, and that profiles are not applied.

**2. Cases added** in `crates/harness-cli/tests/adversary_1105_u3.rs`. Both are red now. Each first observes on disk what `run` did, then compares `tools` with that result.
- `tools_names_the_configured_scope_a_typed_embedded_run_enforces`: precondition held (run rewrote `.b10x/harness.toml`). Red output from running the file alone:
  `panicked at crates/harness-cli/tests/adversary_1105_u3.rs:319:5` … `left: Array [{".git/**" denied}, {".b10x/**" denied}, {"**" allowed}]` / `right: Array [Object {"paths": String("**"), "write": String("allowed")}]`
- `tools_names_the_scope_a_default_write_profile_run_enforces`: preconditions held (`notes.md` written, `.b10x/harness.toml` untouched). Red output:
  `panicked at …adversary_1105_u3.rs:351:5` … `tools with the same flags says the run writes nowhere: [] left: Array [] right: Array [{".git/**" denied}, {".b10x/**" denied}, {"**" allowed}]`
- `test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s`

**3. Gate suites**, run after the cases existed (`cargo test -p b10x-harness-cli --test typed_write_scope --test security_1104_u1 --test repository_profiles --test adversary_1105_u3 --no-fail-fast`, EXIT=101):
- adversary_1105_u3: `test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s`
- repository_profiles: `test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.47s`
- security_1104_u1: `test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s`
- typed_write_scope: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s`

The "before" count of 32 is the unit's reported 9 + 6, plus repository_profiles' 17 from this run (my file is a separate target, so it is not in that count). fmt `--check` exit 0. Clippy `-D warnings` exit 0.

**Mutation results.** Each mutant was applied to a scratch copy (`mut/`, extracted from eb83d8c), and only typed_write_scope and security_1104_u1 were run. Afterwards the copy was restored and `cargo clean --manifest-path` removed 2.3 GiB.

| mutant | result |
|---|---|
| m1: print the typed list instead of the applied one (`write_scope(&options.write_scope)?.rules()`) | killed by 3 tests: `tools_describes_the_scope_a_typed_confined_run_applies`, `tools_names_the_default_scope_a_typed_embedded_run_applies`, `tools_without_confinement_names_an_empty_scope` |
| m2: drop the `if writes` guard (`if true`) | killed, but only by `tools_without_confinement_names_an_empty_scope` |
| m3: hand `published(...)` `Scope::default()` instead of the applied scope (the old behaviour) | survives, and no test can catch it. Publication never reads the scope, which only acts on calls, and `tools` makes none. So it is an equivalent mutant, not a gap in the tests. |

**5. Attacked and could not break**
- Rule order and first-match: the printed rules are `Scope::rules()` in declaration order, the same `Scope` the catalogue matches against.
- Serialization: the key is the catalogue's own `ScopeRule` serde. `partial-only` is kebab-case. Globs print trimmed, as parsed.
- Published catalogue: it does not change between an empty and the default scope, because publication does not depend on scope.
- Existing consumers: no test or doc reads a fixed key set of the `tools` document. README, the confinement guide and the CLI contract (argv only) are unaffected.
- No-confinement case: `run` stays read-only with a typed scope, and `tools` prints `[]`. They agree.

**6. Paths written outside the worktree.** All are in `~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1105/adv-u3/`:
- `builds.tsv`
- `b1.log`, `b2.log`, `m1.log`, `m2.log`, `m3.log`, `fmt.log`, `clippy.log`, plus matching `*.start` and `*.end` files
- `lib.rs.orig`, `lib.rs.m1`, `lib.rs.m2`, `lib.rs.m3`
- `mut/` (14M source copy, its target already cleaned)

The worktree CLI also recorded my session lease (`adv-u3-harness-1105`) in its registry; it is released.

builds.tsv (summed cargo minutes 1.7, wall about 9.3; peak `target/` 2.5G, under the 8G tell line):
```
2026-11-05T02:46:07Z	0.9	1.4	1.9G	cargo test -p b10x-harness-cli --test adversary_1105_u3	101
2026-11-05T02:47:56Z	0.1	0.3	2.0G	cargo test -p b10x-harness-cli --test typed_write_scope --test security_1104_u1 --test repository_profiles --test adversary_1105_u3 --no-fail-fast	101
2026-11-05T02:48:36Z	0.1	0.1	2.0G (scratch copy)	mutant m1: cargo test -p b10x-harness-cli --test typed_write_scope --test security_1104_u1 --no-fail-fast	101
2026-11-05T02:48:39Z	0.1	0.2	2.0G (scratch copy)	mutant m2: same targets	101
2026-11-05T02:48:51Z	0.1	0.2	2.0G (scratch copy)	mutant m3: same targets	0
2026-11-05T02:52:00Z	0.0	~6	2.0G	cargo fmt -p b10x-harness-cli --check (wall is the slot wait)	0
2026-11-05T02:58:04Z	0.4	1.1	2.5G	cargo clippy -p b10x-harness-cli --all-targets --locked -- -D warnings	0
```
Pre-check: `df -h /` showed 70G free. Cargo minutes are cargo's own "Finished" time plus test time.

| scope path from the brief | |
|---|---|
| `applied_write_scope` / `writes_through_substrate` in `crates/harness-cli/src/lib.rs` | confirmed (1580, 1586) |
| `tools_command` | confirmed (4240) |
| `tests/security_1104_u1.rs` `tools_describes_the_scope_a_typed_confined_run_applies` | confirmed |
| `tests/typed_write_scope.rs` (three new cases) | confirmed |

```findings
- file: crates/harness-cli/src/lib.rs
  line: 4263
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: introduced
  message: tools passes None as the configured scope, so under an operator [default] write-scope or write = true its new write_scope key names a scope (default, or empty) other than the one run enforces with the same flags, including .b10x/** denied while run rewrites .b10x/harness.toml (red cases adversary_1105_u3.rs:319 and :351)
```
