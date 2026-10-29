---
format: aep.planning-md/3
id: story:harness-llm-awaits-the-model
kind: story
status: draft
title: harness-llm awaits the llm model and builds no runtime
relations:
- decomposes: epic:async-native-loop
- depends_on: story:async-model-and-tool-ports
scope:
- confidence: cited
  path: crates/harness-llm/src/lib.rs
revision: 6
---
## Outcome

`harness-llm`'s model port implements the asynchronous `ModelPort` by awaiting
`llm_core::Model::turn`, and that path builds no runtime. The synchronous implementation stays
until `story:blocking-facade-keeps-sync-callers` removes it (migrate step).

## Evidence

- `crates/harness-llm/src/lib.rs:370` wraps the model in `BlockingModel::new`, which starts a
  one-worker runtime per client (`llm/crates/llm-blocking/src/lib.rs:63-80`).
- Credential renewal builds a current-thread runtime and, when a runtime is already current, a
  thread for it (`harness-llm/src/lib.rs:474-502`).
- A write builds another current-thread runtime (`harness-llm/src/lib.rs:1441`).
- `impl ModelPort for LlmPort` at `harness-llm/src/lib.rs:539`.

## Acceptance

- `LlmPort` implements `AsyncModelPort`; its turn, the renewal and the write at `:1441` are awaited
  on the caller's runtime.
- A `#[tokio::test(flavor = "current_thread")]` turns the asynchronous port against the crate's
  local fixture endpoint, including one renewal, and completes. (A nested `block_on` in that path
  panics, and `BlockingModel::turn` refuses on a runtime thread, so the test fails if either
  remains.)
- The existing `harness-llm` tests pass.

## Scope

Recorded as `scope` entries. cited: `crates/harness-llm/src/lib.rs`. No dependency changes until the contract story drops `b10x-llm-blocking`.

## Depends on

`story:async-model-and-tool-ports`.
