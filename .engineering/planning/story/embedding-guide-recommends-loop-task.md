---
format: aep.planning-md/3
id: story:embedding-guide-recommends-loop-task
kind: story
status: active
title: The embedding guide recommends LoopTask; the synchronous loop is the fallback
summary: website guide, README link and rustdoc present LoopTask first.
relations:
- decomposes: epic:loop-awaitable-from-async-programs
- depends_on: story:loop-task-awaits-a-run
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: README.md
- confidence: cited
  path: crates/harness-loop/examples/await_a_run.rs
- confidence: cited
  path: crates/harness-loop/examples/blocking_run.rs
- confidence: cited
  path: crates/harness-loop/examples/resume_an_approval.rs
- confidence: inferred
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: website/docs/guides/embedding.md
- confidence: inferred
  path: website/sidebars.ts
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T03:35:24Z", actor: "human:timo", revision: 7, decided_on: {"recorded":{"review_outcome":7}}}
- {from: "proposed", to: "active", at: "2026-10-29T03:35:24Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"review_outcome":7}}}
---
## Outcome

The documentation recommends `LoopTask` as the way to embed the loop. The synchronous
`AgentLoop::run`, `run_in`, `resume_approval` and `call` are documented as the entry for programs
without an async runtime, and the thread under `LoopTask` is named as an interim implementation
that a later release replaces behind the same methods.

## Evidence

- No page shows how to embed the loop as a library: `website/docs/guides/` holds `command-line`,
  `confinement`, `profiles`, `sessions-and-events`, `structured-runs`; `README.md` has no embedding
  section.
- Nothing compiles code blocks in `website/docs` (`crates/harness-docs/src/lib.rs:149`), and
  `cargo xtask gate` runs no website build (`crates/harness-xtask/src/main.rs:77-129`).
- `crates/harness-loop` has no `examples/` directory.

## Acceptance

- Three examples compile in `cargo build -p b10x-harness-loop --examples`:
  `crates/harness-loop/examples/await_a_run.rs` (a tokio `main` that awaits `LoopTask::run`),
  `resume_an_approval.rs` (awaits `run`, then `resume_approval`), and `blocking_run.rs` (a plain
  `main` on `AgentLoop::run`).
- `website/docs/guides/embedding.md` exists with the headings, in this order, `## Await a run`,
  `## Resume an approval`, `## Without an async runtime`. Each of the first two links its example
  file and holds no Rust block over 15 lines. The third names `run`, `run_in`, `resume_approval`
  and `call` and links `blocking_run.rs`.
- The guide contains a sentence stating that `LoopTask` runs the synchronous loop on its own
  thread today and that a later release replaces that thread without changing `LoopTask`'s methods.
- `README.md` links the guide from `## Three shells, one loop` and names `LoopTask` as the
  recommended embedding.
- The guide states that dropping an unfinished `LoopTask` future cancels the task for good: every
  later run on it stops `cancelled`.
- The rustdoc on `LoopTask` carries a doctest that compiles (`no_run`) under
  `cargo test -p b10x-harness-loop --doc`.
- The doc comments of `AgentLoop`, `run`, `run_in`, `resume_approval` and `call` each contain the
  intra-doc link `` [`LoopTask`] ``, five in all (counted with `grep`), and the strict rustdoc
  step of `cargo xtask gate` passes, which fails on a broken intra-doc link.
- `cargo run --locked -p harness-docs -- generate --check` passes.

## Scope

Recorded as `scope` entries.

## Depends on

`story:loop-task-awaits-a-run`.
