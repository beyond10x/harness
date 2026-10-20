---
format: aep.planning-md/3
id: story:a-written-directory-is-judged-by-what-it-holds
kind: story
status: implemented
title: A written directory is judged by what it holds
summary: The write scope refuses a written path naming a directory when it refuses anything that could lie under it.
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T19:05:30Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-20T19:05:31Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-20T22:41:43Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

`review-result:adversary-buildtest-pass-1`, finding 4 (pre-existing): harness's write scope
(`crates/harness-catalogue/src/scope.rs`, the matching at :164) judges a written path by its own
spelling against the rules. A path that names a directory (`protected`, `.`, `protected/.`) is not
matched by a deny rule over that directory's contents (`protected/**=denied`), so a toolchain tool
whose written path is a directory (a formatter given a directory) rewrites files the scope refuses —
locally and under `--substrate-embedded`, where the confined workspace is writable to the program.
Red cases: `crates/harness-catalogue/tests/toolchain_adversary.rs`
`a_written_path_naming_a_directory_that_holds_refused_files_starts_nothing`,
`go_fmt_on_a_directory_rewrites_no_file_the_scope_refuses`, and
`crates/harness-substrate/src/tests.rs`
`a_confined_toolchain_write_naming_the_directory_that_holds_refused_files_sends_no_exec`
(ignored in the build-test unit pending this story).

Decided by the coordinator under the owner's exception 5 (atlas ADR 0097, wave tools-3): fixed in
0.16.0 before the release.

## Outcome

A written path that names a directory is refused when the scope refuses anything that could lie
under it, so no write through a directory reaches a file the scope denies.

## Acceptance

- The three red cases above pass with their `#[ignore]` removed.
- A written directory none of whose possible contents a rule denies is admitted as before; a
  written file is judged exactly as before (existing scope tests unchanged and green).
- The refusal names the directory and the rule it would reach past.
- `CHANGELOG.md` lists it under Fixed in one line, without a reproduction.
- `cargo xtask gate` exits 0; argv byte-identical.

## Scope

- `crates/harness-catalogue/src/scope.rs` (the judgement of a written path), its tests, and the
  callers that pass written paths (`crates/harness-catalogue/src/catalogue.rs` `refusing_writes`,
  `Catalogue::refusal`) only if their call shape must change.
- The three ignored cases (remove `#[ignore]`), `CHANGELOG.md`.

## Out of scope

- A written path filled from a spec's default (finding 3): upstream in `tool-toolchain`'s
  `plan.writes`.
- Any change to how rules are written or to `--write-scope`'s syntax.
