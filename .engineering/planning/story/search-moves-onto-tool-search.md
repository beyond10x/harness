---
format: aep.planning-md/3
id: story:search-moves-onto-tool-search
kind: story
status: implemented
title: Harness's search is served by b10x-tool-search
summary: LocalOperations::search calls tool_search (tools 0.3.0); harness's grep and walk code is deleted; reply changes from tools 0.3.0 listed in the CHANGELOG.
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T14:22:20Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-20T14:22:20Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-20T22:41:43Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

harness's `search` is still served by its own code: `LocalOperations::grep`
(`crates/harness-catalogue/src/local.rs:258-374`) with its helpers `search_expression`, `cut_to`,
`around`, `PathGlob`, `walk` and `contained` (`local.rs:589-858`), the half of the old
`harness-tools/src/local.rs` that tools-1 kept local because tools `0.1.0` had no search toolset.
Search is always the local reader's: the confined provider refuses it
(`crates/harness-substrate/src/tools.rs:404`) and `harness_catalogue::Split` routes it to the reader.

tools `0.3.0` (`e89380d8`, verified by `b10x-chef-orchestrator`) ships `search` v1alpha1
(`search.grep`) and `b10x-tool-search`, "harness 0.14.1's `grep` moved here", over
`b10x-tool-fs`'s now-public `tool_fs::walk`. Atlas ADR 0097 §3: harness consumes the tools crates.
Approved for 0.16.0 by `b10x-chef-orchestrator` under the owner's exception 5.

## Outcome

harness's `search` is served by `b10x-tool-search` through the `crates/harness-catalogue` adapter, as
`find` is served by `b10x-tool-fs`; harness's own grep and walk code is gone.

## Acceptance

- `LocalOperations::search` calls `tool_search::Search::grep`; `search → search.grep` joins the
  adapter's mapping table (`local.rs` `MAPPING`), tested both ways.
- `grep`, `search_expression`, `cut_to`, `around`, `PathGlob`, `walk` and `contained` are deleted
  from `local.rs`, or kept only where another operation still uses them (named in the report).
- The model-facing `search` entry (name, input schema, summary) is unchanged unless a field changes
  meaning; every reply change is listed in `CHANGELOG.md` with its cause, at least those tools
  `0.3.0` names against harness 0.14.1's grep: `truncated: false` at exactly `max_results`
  matches; an empty `glob` and a `path` that climbs out of the workspace refused; a named pipe
  passed over; an entry named like a skipped directory (a worktree's `.git` file) not searched; a
  file holding a NUL byte treated as binary and not searched; `skipped_large_paths` beside
  `skipped_large_files`.
- tools pinned at tag `0.3.0` (`e89380d8`) in `[workspace.dependencies]` (invariant 2), with
  `b10x-tool-search` added; every tools crate moves together.
- `cargo xtask gate` exits 0; argv byte-identical (`cargo xtask cli-contract`).

## Scope

- `Cargo.toml` (`[workspace.dependencies]`: four tools crates to `0.3.0`, `tool-search` added),
  `Cargo.lock`, `crates/harness-catalogue/Cargo.toml`.
- `crates/harness-catalogue/src/local.rs` (search, mapping, deleted helpers), `src/lib.rs` docs,
  `src/catalogue.rs` only if the `search` entry's description must change.
- Tests that pin search replies: `crates/harness-catalogue/src/tests.rs`, `local.rs` tests,
  `crates/harness-substrate/tests/conformance.rs` where it compares search across providers.
- `CHANGELOG.md`, `STATUS.md` workspace-tools row, `README.md` foundations pin text.

## Out of scope

- `run` and the toolchain (`story:run-moves-onto-tool-process`, and the build-test move, which waits
  on a decision about the executor's runner).
- Publishing `search.grep`'s own name to the model (harness keeps `search`).
