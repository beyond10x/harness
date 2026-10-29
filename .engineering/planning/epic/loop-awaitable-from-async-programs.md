---
format: aep.planning-md/3
id: epic:loop-awaitable-from-async-programs
kind: epic
status: active
title: An async program awaits the loop through LoopTask, the documented default
summary: LoopTask owns a run's ports and returns runtime-neutral futures for run, run_in, resume_approval and call; the synchronous AgentLoop stays for programs without a runtime.
relations:
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T03:35:19Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"review_outcome":7}}}
- {from: "proposed", to: "active", at: "2026-10-29T03:35:20Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"review_outcome":7}}}
---
## Evidence

Read on 2026-10-29 at `e5ab71d` of this repository (workspace version `0.22.0`).

- `AgentLoop<'a>` borrows every port (`crates/harness-loop/src/lib.rs:1805-1811`) and its public
  entry points block the calling thread: `call` (`lib.rs:2120`), `run` (`:2148`), `run_in`
  (`:2189`), `resume_approval` (`:2227`).
- `LlmPort::turn` already runs `BlockingModel::turn` on a scoped std thread with no runtime context
  (`crates/harness-llm/src/lib.rs:558-565`), so the model does not refuse a loop started from
  `spawn_blocking`. The previous epic's evidence said it does; that claim was false
  (`review-result:async-loop-adversary-feasibility`).
- harness-mcp and harness-substrate call `Runtime::block_on` on private runtimes
  (`crates/harness-mcp/src/lib.rs:202`, `crates/harness-substrate/src/embedded.rs:315-492`). On a
  thread with no runtime context that is legal; on a runtime worker or inside a root `block_on` it
  panics (feasibility review probe). Not yet run end to end from an async program.
- harness-loop has no tokio dependency (`crates/harness-loop/Cargo.toml`); cancellation is
  `harness_wire::Cancel`, an `Arc<AtomicBool>` (`crates/harness-wire/src/cancel.rs:13`,
  `lib.rs:351`).
- Two adversarial reviews found the async-core plan (`epic:async-native-loop`, seven stories)
  not implementable as written: 4 blockers, 21 majors
  (`review-result:async-loop-adversary-feasibility`, `review-result:async-loop-adversary-blast-radius`).
- Requirement (Timo, 2026-10-29): internal libraries on tokio embed the loop. Decision (Timo,
  2026-10-29): option A, and "make async the default and recommendation. a sync adapter is a
  workaround".

## Outcome

A program on any async runtime awaits a whole run, a resumed approval or a single gated call
through one handle, `LoopTask`, without starting a thread or runtime itself and with no API break
for today's synchronous callers. The documentation presents `LoopTask` as the default way to embed
the loop; the synchronous `AgentLoop` methods are documented as the entry for programs without an
async runtime.

## Decisions taken

- **D1 — the ports live on the task's own thread, from construction to drop.** `LoopTask::spawn`
  takes the `LoopConfig` and a builder closure (`FnOnce(&LoopCancel) -> Result<LoopPorts, _> +
  Send + 'static`) and runs the builder on a `std::thread` it starts. The model, tool and approval
  ports, and the optional hooks and environment, are built, used and dropped on that thread and
  never cross to the caller. Reasons: borrowed ports cannot cross threads (E0521, feasibility
  review); `McpTools::prepare` calls `Runtime::block_on` (`crates/harness-mcp/src/lib.rs:117,129`)
  and `McpTools` and `Embedded` own a tokio `Runtime` whose drop panics in an async context
  (`review-result:awaitable-design-round-1`, probe on tokio 1.53.2). The ports need not be `Send`.
- **D2 — no runtime context on that thread.** `BlockingModel` does not refuse there and the
  private `block_on` calls in harness-mcp and harness-substrate do not nest. This is the workaround
  Timo named: the loop's internals stay synchronous until `epic:async-native-loop` replaces them
  behind the same `LoopTask` methods.
- **D3 — runtime-neutral futures.** harness-loop gains no tokio dependency outside
  `[dev-dependencies]`. `run`, `run_in`, `resume_approval` and `call` send one command to the
  thread when called and return a future the thread completes through a waker; commands run one
  at a time in the order sent. Each takes a sink `S: LoopSink + Send + 'static` by value and
  returns it with the result, so a `VecLoopSink` comes back holding the run's events.
- **D4 — one cancel token per task, with the synchronous loop's semantics.** `LoopTask` creates
  the `LoopCancel`, installs it on every `AgentLoop` it builds (`with_cancel`,
  `crates/harness-loop/src/lib.rs:2081`), passes it to the builder, and exposes it as
  `cancel_handle()`. A port that checks its own token, such as `LlmPort`
  (`crates/harness-llm/src/lib.rs:529,546`), holds the same one. Dropping an unfinished future
  cancels the token. A run on a cancelled token ends as the synchronous loop's does, `Ok` with stop
  `cancelled` (`lib.rs:2672-2673`); `harness_wire::Cancel` has no reset
  (`crates/harness-wire/src/cancel.rs:15-24`), so every later run on that task ends that way.
- **D5 — additive.** No synchronous signature changes and no new `LoopError` variant. The futures
  resolve to `Result<_, LoopTaskError>`, a new type: the loop's own `LoopError`, the builder's
  failure, a panic on the task thread, or a task whose thread has gone. The next minor release
  (0.23.0) lists both under Added.
- **D6 — spawning and dropping do not block.** `spawn` returns at once; a builder failure resolves
  the first command's future. Dropping the `LoopTask` closes the command channel and the thread
  drops the ports and exits on its own. `shutdown(self)` is the awaitable form: it resolves once
  the thread has dropped the ports, and to an error if a drop panicked.

## Done When

- The stories under this epic are implemented and merged in one wave PR.
- `cargo xtask gate` passes on the integration branch.
- `README.md` and `website/docs` show `LoopTask` first wherever they show how to embed the loop.

## Out of scope

- Asynchronous port traits, removal of the private runtimes, an asynchronous app server
  (`epic:async-native-loop`).
- Moving metaharness (including `metaharness-tools`), loom and takt to async by default. Each is
  planned in its own repository's store (Timo, 2026-10-29: "the same becomes true for:
  metaharness, metaharness-tools, loom, takt").
- uilab, agentide and agent-platform: no priority (Timo, 2026-10-29). They keep working unchanged.
- The organisation rule "all I/O is async" is an Atlas ADR, not this epic.
