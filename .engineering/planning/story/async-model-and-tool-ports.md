---
format: aep.planning-md/3
id: story:async-model-and-tool-ports
kind: story
status: archived
title: Asynchronous model, tool and stream ports exist beside the synchronous ones
relations:
- decomposes: epic:async-native-loop
scope:
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: crates/harness-loop/Cargo.toml
- confidence: inferred
  path: crates/harness-loop/src/adapt.rs
- confidence: inferred
  path: crates/harness-loop/src/lib.rs
- confidence: inferred
  path: crates/harness-wire/src/lib.rs
- confidence: cited
  path: crates/harness-wire/src/port.rs
revision: 6
transitions:
- {from: "draft", to: "archived", at: "2026-10-29T03:13:27Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"review_outcome":2}}}
---
## Outcome

`harness-wire` declares asynchronous counterparts of `ModelPort`, `ToolPort` and `StreamSink` beside
the synchronous ones, and an adapter turns any synchronous implementation into the asynchronous
trait. Nothing that implements or calls the synchronous traits changes, so the workspace builds
after this story alone (expand step).

## Evidence

- `crates/harness-wire/src/port.rs:43` (`StreamSink::emit`), `:91` (`ModelPort::turn`, `fork`),
  `:135` (`ToolPort::call`, `call_within`, `call_batch`, `fork`) are synchronous.
- The loop is the only caller of these methods: `model.turn` at
  `crates/harness-loop/src/lib.rs:2658`, `:3080`; `tools.call_batch` at `:4194`; `tools.call_within`
  at `:4392` (design critic, round 1). Changing the traits in place would break it.
- `crates/harness-wire/Cargo.toml` lists no tokio today.

## Acceptance

- `AsyncModelPort::turn`, `AsyncToolPort::call`, `call_within`, `call_batch` and
  `AsyncStreamSink::emit` return `Pin<Box<dyn Future<Output = …> + Send + '_>>`; the descriptive
  methods mirror the synchronous traits and stay synchronous.
- `cargo tree -p b10x-harness-wire -e normal` lists no `tokio`.
- `harness-loop` exposes `Blocking::new(port)` for a synchronous `ModelPort` and for a synchronous
  `ToolPort`, implementing the asynchronous trait by running each call on tokio's blocking pool.
- A `#[tokio::test(flavor = "current_thread")]` turns a synchronous fixture model through the
  adapter while a second task increments a counter every millisecond; the fixture sleeps 50 ms and
  the counter has advanced when the turn returns.
- `cargo test --workspace --locked` passes with no file outside the Scope list changed.

## Scope

Recorded as `scope` entries. cited: `crates/harness-wire/src/port.rs`. inferred:
`crates/harness-wire/src/lib.rs`, `crates/harness-loop/src/adapt.rs` (new),
`crates/harness-loop/src/lib.rs` (module declaration), `crates/harness-loop/Cargo.toml`,
`Cargo.toml`, `Cargo.lock`.

## Out of Scope

Every existing implementor; each moves in its own story and the synchronous traits are renamed in
`story:blocking-facade-keeps-sync-callers`.
