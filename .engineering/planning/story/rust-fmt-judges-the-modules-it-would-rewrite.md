---
format: aep.planning-md/3
id: story:rust-fmt-judges-the-modules-it-would-rewrite
kind: story
status: implemented
title: rust_fmt judges every module file it would rewrite
summary: 'Before rustfmt runs, harness resolves the named files'' mod and #[path] declarations and judges each module file as a written file; unresolvable declarations refuse.'
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T20:34:42Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-20T20:34:42Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-20T22:41:43Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

`review-result:adversary-scope-pass-1`, finding 3 (pre-existing): the built-in `rust_fmt` runs
`rustfmt` on the named files, and `rustfmt` also rewrites every module those files declare — `mod x;`
resolved to `x.rs` or `x/mod.rs`, and `#[path = "…"]` modules anywhere the attribute points — while the
write scope judges only the named files. A file the scope refuses is reformatted when a file it admits
declares it as a module. Red case: `crates/harness-catalogue/tests/written_directory_adversary.rs`
`rust_fmt_on_a_named_file_rewrites_no_module_file_the_scope_refuses` (ignored pending this story).

Decided by `b10x-chef-orchestrator` (2026-10-20, option A): fixed in 0.16.0; the second advisory
claims the write-scope class fixed, so a known member must not ship.

## Outcome

Before `rust_fmt` (and any toolchain operation that runs `rustfmt` on named files) starts, harness
resolves the named files' module declarations recursively and judges every module file they reach as a
written file; a declaration it cannot resolve is refused, so nothing a scope refuses is reformatted.

## Acceptance

- The red case above passes with its `#[ignore]` removed.
- `mod x;` (to `x.rs` and `x/mod.rs`, from a `mod.rs`/`lib.rs`/`main.rs` and from a non-root file),
  inline `mod x { mod y; }`, and `#[path = "…"]` on a module (relative to the declaring file, as
  rustfmt resolves it) are followed; each resolved file is judged as a written file, where it lands.
- Fail-closed: a declaration whose target cannot be determined statically (`#[path]` built by a macro,
  `#[cfg_attr(…, path = …)]`, a target outside the workspace, a file that cannot be read) refuses the
  call naming the declaring file and line; the walk is bounded (files and depth, named constants).
- With no refusing rule in the scope, nothing is scanned and `rust_fmt` behaves as before.
- Refusals end with what to do instead (name the files to format, none of which declares a refused
  module).
- `CHANGELOG.md` gains no new line beyond the existing Fixed line if it already covers it; otherwise one
  behaviour line without mechanism.
- `cargo xtask gate` exits 0; argv byte-identical.

## Scope

- `crates/harness-catalogue` (a module scanner beside `written.rs`, called from the toolchain closure
  for operations whose program is `rustfmt`), its tests, the ignored case, `CHANGELOG.md`.

## Out of scope

- Changing the `rust_fmt` spec or how `rustfmt` is invoked (the tools repository).
- Other formatters that follow includes (none among the built-ins).
