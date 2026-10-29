---
format: aep.planning-md/3
id: epic:async-native-loop
kind: epic
status: draft
title: A tokio program awaits the loop, and a synchronous caller still blocks on it
summary: Ports and AgentLoop::run become asynchronous; a blocking facade keeps today's API; Harness stops building private runtimes over llm, mcp and Substrate.
relations:
- serves: vision:b10x-owns-its-loop
- informed_by: epic:embedded-by-a-consumer
revision: 2
---
## Evidence

Read on 2026-10-29 at `origin/main` of this repository (workspace version `0.22.0`, `Cargo.toml:18`).

- Every port the loop calls is synchronous: `StreamSink` (`crates/harness-wire/src/port.rs:43`),
  `ModelPort` (`port.rs:91`), `ToolPort` (`port.rs:135`), `ApprovalPort`
  (`crates/harness-loop/src/approval.rs:42`), `TurnEnvironmentProvider`
  (`crates/harness-loop/src/environment.rs:47`), `HookPort` (`crates/harness-loop/src/hook.rs:139`),
  `LoopSink` (`crates/harness-loop/src/event.rs:399`).
- `AgentLoop::run` and `run_in` block the calling thread (`crates/harness-loop/src/lib.rs:2148`,
  `:2189`). Side-by-side delegates run on `std::thread::scope` (`lib.rs:3918`) over forked ports
  (`lib.rs:3900`).
- Every dependency below the loop that does I/O is already asynchronous on tokio, and Harness
  blocks on each through a private runtime:
  - `llm`: `BlockingModel::new` (`crates/harness-llm/src/lib.rs:370`), which owns a one-worker
    runtime; credential renewal builds another (`harness-llm/src/lib.rs:494`, `:502`) and a
    third for a write (`harness-llm/src/lib.rs:1441`).
  - `mcp`: a current-thread runtime per connection (`crates/harness-mcp/src/lib.rs:112`).
  - Substrate: a current-thread runtime per embedded backend
    (`crates/harness-substrate/src/embedded.rs:163`).
- `llm-blocking`'s `BlockingModel::turn` refuses whenever `Handle::try_current()` is `Ok`
  (`llm/crates/llm-blocking/src/lib.rs:132`). A probe on tokio 1.54.1, run 2026-10-29, printed
  `task=true spawn_blocking=true std_thread=false`: a `tokio::task::spawn_blocking` thread sees a
  handle, so every model turn of a loop started from `spawn_blocking` returns `Unsupported`. Only a
  plain `std::thread` can host the loop today.
- Consumers that implement Harness ports outside this repository (grep for `impl … Port for`,
  2026-10-29): `uilab` 8 files, `agentide` 2, `agent-platform` 2, `control-plane` 1.
  `metaharness-b10x` depends on `b10x-harness-loop` (`metaharness/crates/metaharness-b10x/Cargo.toml:18`).
- Requirement (Timo, 2026-10-29): internal libraries that run on tokio want to embed the loop.

## Outcome

A program on a tokio runtime awaits `AgentLoop::run` from an ordinary task, with no thread of its
own and no runtime nested inside Harness. A synchronous caller keeps a blocking entry point with
the behaviour it has today.

## Decisions taken

- **D1 — one asynchronous implementation, one blocking facade.** The loop is written once, async.
  The blocking API is a thin adapter over it, the shape `llm` already uses (`llm-core` async,
  `llm-blocking` adapter). Two hand-maintained loops would drift.
- **D2 — ports return boxed futures, `Send`.** The ports are used as trait objects
  (`&mut dyn ModelPort`, `lib.rs:1805`), so they return `Pin<Box<dyn Future + Send>>`, as
  `llm_core::BoxFuture` does. `Send` lets a consumer `tokio::spawn` a run.
- **D3 — `harness-wire` stays runtime-neutral.** It names no executor and gains no tokio
  dependency (AGENTS.md invariant 3: no I/O, no clock). Tokio appears only in adapters and in the
  crates that already depend on it.
- **D4 — synchronous ports stay implementable.** An adapter turns a synchronous `ModelPort`,
  `ToolPort` or loop port into the asynchronous one by running it on the blocking pool, so each of
  the 13 consumer files migrates by wrapping, not by rewriting.
- **D5 — expand, migrate, contract.** Asynchronous traits and `run_async` land beside the
  synchronous ones; each crate then gains its asynchronous path; a last story renames, deletes the
  synchronous duplicates and releases. Every story leaves `cargo test --workspace` green.
- **D6 — the catalogue's `Operations` trait becomes asynchronous**
  (`crates/harness-catalogue/src/operations.rs:196`), so the confined backend is awaited, and the
  local implementation runs its synchronous calls on the blocking pool.

## Stories, in order

1. `story:async-model-and-tool-ports` — expand: asynchronous model, tool and stream traits and the
   blocking-pool adapters.
2. `story:loop-runs-as-a-future` — expand: the loop as a future; `run`/`run_in` become its blocking
   facade.
3. `story:harness-llm-awaits-the-model`, `story:catalogue-awaits-its-operations` — migrate, in
   parallel once 1 has landed.
4. `story:mcp-and-substrate-await-their-drivers` — migrate, after the catalogue story.
5. `story:binary-and-catalogue-on-one-runtime` — migrate the binary, app server and conformance
   target.
6. `story:blocking-facade-keeps-sync-callers` — contract and release.

## Scope

`harness-wire`, `harness-loop`, `harness-llm`, `harness-mcp`, `harness-substrate`,
`harness-catalogue`, `harness-cli`, `harness-app-server`, `harness-conformance` and the
specification target in `harness-xtask`.

## Out of Scope

- Changing `llm-blocking`'s runtime guard. That is the `llm` repository's work; this epic removes
  Harness's use of `BlockingModel` instead.
- Making the `tools` repository's `Toolset::invoke` asynchronous
  (`tools/crates/tool-contract/src/conformance.rs:568`). Filesystem and process tools stay
  synchronous and run on the blocking pool.
- Migrating consumers in other repositories. Each consumer moves to the release in its own
  repository.
- Any change to the CLI's argv contract under `contracts/cli/` or to the wire format.

## Done When

- A test in this repository awaits `AgentLoop::run` inside `#[tokio::main]` from a
  `tokio::spawn`ed task, with a model, a tool call and an approval, and passes.
- The blocking entry point passes the existing `harness-loop` suite unchanged in behaviour.
- `grep -rn 'new_current_thread\|new_multi_thread\|block_on\|BlockingModel' crates/harness-llm/src crates/harness-mcp/src crates/harness-substrate/src`
  returns nothing. The blocking facade's own runtime lives in `crates/harness-loop`, outside these
  paths.
- `cargo xtask specification` passes with the conformance target driving the asynchronous loop.
- The release is `0.23.0`, with the port change under a `Breaking` heading in `CHANGELOG.md`
  (`story:blocking-facade-keeps-sync-callers`).
