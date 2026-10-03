---
format: aep.planning-md/3
id: task:current-rust-gate-compatibility
kind: task
status: implemented
title: Keep the source gate green on current stable Rust
relations:
- decomposes: epic:gate-stays-trustworthy
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-03T19:57:54Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"test_result":1}}}
- {from: "proposed", to: "active", at: "2026-10-03T19:57:54Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}}
- {from: "active", to: "implemented", at: "2026-10-03T19:57:54Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}}
---
## Evidence

The session migration baseline passed workspace tests on Rust 1.99.0, then failed clippy::assert_is_empty in harness-flow and harness-tools. Rust 1.99 also deprecates AtomicUsize::fetch_update, while the repository's minimum remains Rust 1.97.

## Acceptance

`cargo xtask gate` passes with current stable Rust without raising rust-version or changing observable runtime behavior; assertions retain their checks and the atomic update remains compatible with Rust 1.97.

## Migration prerequisite

The home-path check must inspect both the staged index and the worktree after migration. Port its existing byte scanner and planted-fixture self-test from Python to Rust, as required on a material change, and carry the prior journal exception only to the exact extracted historical evidence path with a content hash. This neither rewrites old evidence nor exempts newly authored reports. Newly authored public reviews carry explicitly redacted local checkout prefixes.
