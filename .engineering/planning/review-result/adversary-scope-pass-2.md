---
format: aep.planning-md/3
id: review-result:adversary-scope-pass-2
kind: review-result
status: active
title: Adversary pass 2 on units 4 and 5 of wave tools-3 (written directories, rust_fmt modules)
relations:
- reviews: story:rust-fmt-judges-the-modules-it-would-rewrite
revision: 1
---
U4+U5 adversary pass 2 (units 4 at 2275ca31 and 5 uncommitted, over ae6e45bc): no blocker; 1 red case, 2
warnings, 2 notes.

Added `crates/harness-catalogue/tests/scope_class_adversary.rs`, 7 cases, six through real rustfmt 1.10.0:
non-root fallback, a cfg-gated module, a unicode-named module, a symlinked declaring file, three non-roots
deep, and this crate's own source under the default profile — all green; one `rust_build` case red.

Findings:

1. `crates/harness-catalogue/src/catalogue.rs:562` (warning, CONFIRMED, pre-existing, in class) —
   toolchain tools that write the workspace but declare no `writes:` (build, test, clippy, bench, run,
   fetch; go peers) run programs whose build scripts, proc macros or tests can write scope-refused
   paths the scope never judges. By AGENTS.md design these are Risk::High process calls gated by
   approval and confinement, not by the scope; the advisory's claim must be scoped to tools that
   declare their written paths. Red: `rust_build_writes_a_scope_denied_path_through_a_build_script_the_scope_never_judged`.
2. `crates/harness-catalogue/src/modules.rs:642` (warning, INFEASIBLE, pre-existing, in class) — a
   module file hard-linked to a refused file is judged at its own name; the stated hard-link gap,
   extended to module files by unit 5.
3. `crates/harness-catalogue/src/modules.rs:466` (note) — over-refusals confirmed against rustfmt
   (crate-level `#![path]`, both module files present, `mod` in a function body): all refuse more.
4. `crates/harness-catalogue/src/modules.rs:166` (note) — the module scan runs before the call's
   deadline, bounded by bytes only; wall time and peak memory not measured.

Attacked and could not break: module resolution against rustfmt 1.10.0 on about 35 fixtures (every rule;
`#[path]` forms, inline modules, roots, raw and unicode names, symlinked declaring file, cfg, cfg_attr,
cfg_if, inner path, include!, doc-comment and raw-string `mod`, BOM, CRLF) — matches or refuses more;
no over-refusal on this crate's source; no stack overflow (iterative lexer and walks); unit 4 link
cycles, many links to one tree, a link to the written directory's parent; argv byte-identical;
proc-macro2 added through `[workspace.dependencies]` with no new package.

```findings
[{"file":"crates/harness-catalogue/src/catalogue.rs","line":562,"category":"judgement","severity":"warning","verdict":"CONFIRMED","origin":"pre-existing","message":"Workspace-writing toolchain tools that declare no writes: run programs that can write scope-refused paths the scope never judges; by design Risk::High behind approval and confinement; the advisory claim must be scoped to declared-write tools."},
{"file":"crates/harness-catalogue/src/modules.rs","line":642,"category":"boundary","severity":"warning","verdict":"INFEASIBLE","origin":"pre-existing","message":"A module file hard-linked to a refused file is judged at its own name; the stated hard-link gap, extended to module files."},
{"file":"crates/harness-catalogue/src/modules.rs","line":466,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"Over-refusals confirmed against rustfmt all refuse more; fail-closed."},
{"file":"crates/harness-catalogue/src/modules.rs","line":166,"category":"boundary","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"The module scan runs before the call's deadline, bounded by bytes only; wall time and memory not measured."}]
```
