---
format: aep.planning-md/3
id: review-result:async-loop-adversary-feasibility
kind: review-result
status: active
title: 'Adversarial plan review: technical feasibility, async-native loop'
relations:
- reviews: epic:async-native-loop
- reviews: story:async-model-and-tool-ports
- reviews: story:loop-runs-as-a-future
- reviews: story:harness-llm-awaits-the-model
- reviews: story:catalogue-awaits-its-operations
- reviews: story:mcp-and-substrate-await-their-drivers
- reviews: story:binary-and-catalogue-on-one-runtime
- reviews: story:blocking-facade-keeps-sync-callers
prose_only: adversarial plan review; findings are a severity-marked prose list
revision: 1
---
verdict: unsound

**Findings**

- story:loop-runs-as-a-future — blocker, verified — `run`/`run_in` cannot wrap their ports with story 1's spawn_blocking adapter. The ports arrive borrowed (`&'a mut dyn ModelPort`, `lib.rs:2049-2051`; the tests pass `&mut dyn` at `tests.rs:6576-6583`), so they are neither `'static` nor `Send`. A probe `spawn_blocking(move || port.turn())` with `port: &mut (dyn Port + Send)` fails with E0521 "borrowed data escapes outside of function". `unsafe_code = "forbid"` (`Cargo.toml:83`) rules out a lifetime escape.
- story:loop-runs-as-a-future — blocker, verified — the acceptance set cannot all hold at once: no `thread::scope` in `lib.rs`, an unchanged `tests.rs`, and the story-1 adapters.
  - `two_delegates_of_one_turn_run_at_the_same_time` (`tests.rs:6603`, peak==2 asserted at `:6617`) needs two synchronous `ForkedModel::turn` calls busy-waiting at once (`tests.rs:6354-6362`, `:6479`). That takes two OS threads.
  - The forks are `Box<dyn ModelPort + Send + '_>` (`port.rs:126`, `lib.rs:3896`), so neither `spawn_blocking` nor `tokio::spawn` can take them.
  - Child futures joined on one task with inline synchronous calls run one after the other. Probe: an inline 50 ms block left a 1 ms counter task at 0.
  - Fix: a child-executor seam (async: join; facade: scoped threads each blocking on one child), or `'static` forks.
- epic D5 / story ordering — blocker, verified — after story 2 the facade blocks on a tokio runtime and must call borrowed synchronous ports inline.
  - `McpTools::call_within` calls `self.runtime.block_on` (`harness-mcp/src/lib.rs:202`); the embedded Substrate backend does the same (`embedded.rs:315-492`).
  - Probe: a nested `Runtime::block_on` fails inside a task and inside a root `block_on` (`ct_task nested_block_on_ok=false`, `mt_block_on_root …=false`).
  - So `end_to_end.rs:324 outbound_mcp_runs_end_to_end_over_stdio` should go red until stories 4–5. That this test fails is inferred; I did not run it.
  - Fix: the facade runs each synchronous call on a scoped std thread with no tokio context, as `LlmPort` already does (`harness-llm/src/lib.rs:558-565`), or blocks with an executor that is not tokio.
- story:catalogue-awaits-its-operations — major, verified (code) / inferred (consequences) — the story changes `Operations` in place (no expand step) yet keeps the synchronous `Flat`/`Verbs` `ToolPort`.
  - That synchronous path must bridge to the async trait. A `block_on` there panics under the facade, and `spawn_blocking` panics in a plain `#[test]` with no runtime.
  - `ConfinedOperations` (`tools.rs:153`) has to change in this same story, although story 4 claims it.
  - `invoke_batch` keeps per-thread panic containment (`catalogue.rs:685-720`); the async replacement is not described.
  - `Box<dyn Operations>` with `&self` methods (`catalogue.rs:75`) means `LocalOperations` needs `Arc`/clone of its state before `spawn_blocking` can work.
- story:loop-runs-as-a-future — major, verified — the facade cannot tell a runtime worker from a `spawn_blocking` thread through tokio's public API.
  - Probe: `try_current` and `task::try_id` are both true on both kinds of thread.
  - The only signal that differs is tokio's nested-runtime panic. The "named error" therefore needs `catch_unwind` around a throwaway `block_on`, which is noisy and dead under `panic=abort`. The plan should state the mechanism.
- epic Evidence + story 2 Evidence — major, verified — the claim "a loop started from spawn_blocking gets Unsupported; only a plain std::thread can host it" is false for `LlmPort`.
  - `LlmPort` runs `BlockingModel::turn` on a fresh scoped std thread (`harness-llm/src/lib.rs:558-565`). Probe: `try_current=false` there.
  - The cited probe ran on tokio 1.54.1, but `Cargo.lock:4695` pins 1.53.1.
- story:harness-llm-awaits-the-model — major, verified — the test can pass with the old path still in place.
  - The parenthetical "fails if either remains" is wrong: the turn already escapes to a scoped thread (`lib.rs:558`), and renewal escapes when a runtime is current (`lib.rs:502-509`, already tested at `tests/adversary_public_surface.rs:487`).
  - Renewal is not part of the port; it is called from `harness-cli/src/lib.rs:1601`.
  - `:1441` is test-fixture code inside `#[cfg(test)] mod tests` (`lib.rs:854`), not a production write.
- story:mcp-and-substrate-await-their-drivers — major, verified — the Barrier test is not implementable as written and would not discriminate.
  - `Embedded` wraps the concrete `substrate_host::HostDriver` (`embedded.rs:52`, `:81`), so there is no read handler to inject a Barrier into.
  - A current-thread `block_on` called from a non-core thread still polls its own future (`tokio-1.53.1 …/current_thread/mod.rs:220-235`). Probe: two threads blocking on one current-thread runtime and waiting on `Barrier(2)` finished in 1 ms.
  - With no timeout, a regression hangs instead of failing.
- story:binary-and-catalogue-on-one-runtime — major, verified — the app server has no runtime and no tokio dependency (`harness-app-server/Cargo.toml`).
  - `serve` is synchronous and thread-based (`lib.rs:78`, `transport.rs:78`), and `ModelFactory` returns a non-`Send` `Box<dyn ModelPort>` (`lib.rs:70`).
  - "Await without block_on" therefore means making `serve` async: a public API break, `transport.rs` is outside the story's scope, and the release notes don't list it.
  - The app server calls `run`, not `run_in` (`lib.rs:589`).
- whole plan — major, verified — `AgentLoop::call` (`lib.rs:2120`) and `resume_approval` (`lib.rs:2227`) are public synchronous entry points that drive the ports. No story gives them an async form or a facade, and the conformance `harness.loop.Call` uses `.call` (`boundary.rs:160-161`).
- story:blocking-facade-keeps-sync-callers — major, verified (scope) / inferred (consumers) — the scope omits `harness-loop/src/tests.rs` (131 run/run_in sites), `harness-catalogue/tests` and `harness-substrate/tests`.
  - After the rename these need `Blocking` wrappers, and those need owned `'static + Send` ports (see the first finding).
  - The promise "a sync consumer keeps working" has no test, and it does not hold for borrowed ports.
- story:loop-runs-as-a-future — minor, verified — "Send when those ports are" cannot be conditional over `dyn` fields (`lib.rs:1806-1811`).
  - Probe: a future holding `&mut dyn Port` gives "future cannot be sent between threads safely".
  - The traits need `: Send` supertraits, as llm-core has (`llm-core/src/port.rs:60`, `:65`), and that forces the CLI's `Renderer`, `Terminal` and `Caller` to be `Send`.
- story:async-model-and-tool-ports — minor, verified (llm-core) / inferred (compile) — the signature `+ Send + '_` on `&mut self, &TurnRequest, &mut dyn sink` binds the future to `self` only. A future that captures `request` or `sink` won't compile. It needs one explicit `'a` (as `llm-core/src/port.rs:75-80`) and a `dyn AsyncStreamSink + Send` sink.
- story:async-model-and-tool-ports — minor, inferred — the adapter's costs and drop behaviour are unspecified.
  - It needs `P: Send + 'static`, a clone of the TurnRequest and ToolCall on every call, and a channel to carry sink events across.
  - A dropped future loses the port and leaves the synchronous call running, while story 2 tests dropping the future.
- story:loop-runs-as-a-future — minor, verified — the scope and dependencies are incomplete.
  - `parallel.rs` blocks on std mpsc `recv` (`:80`, `:218`) and is not in scope.
  - The backoff uses `std::thread::sleep` (`lib.rs:2714`).
  - Async channels need tokio `sync`, which the workspace features lack (`Cargo.toml:59`: rt, time, macros), so "no dependency changes" is wrong.
  - Catching a child's panic (`lib.rs:3936`) needs a future-level `catch_unwind`.
- epic Done When — minor, inferred — the default `#[tokio::main]` flavour needs `rt-multi-thread`, which is not among the workspace tokio features (`Cargo.toml:59`).
- story:binary-and-catalogue-on-one-runtime — minor, verified — three citations point at `use` lines: `approve.rs:25`, `caller.rs:53`, `hooks.rs:58`. The impls are at `:140`, `:369`, `:613`. `Renderer` (`render.rs:219`) and `BridgeSink` (`session.rs:253`) implement `LoopSink` but are not listed, and `render.rs` is outside the scope.
- story:mcp-and-substrate-await-their-drivers — minor, verified — the `block_on` citations are off by one; the actual lines are `embedded.rs:315,344,390,463,481,492`.
- story:mcp-and-substrate-await-their-drivers — minor, inferred — connections and the driver built on the private runtime cannot be awaited on the caller's runtime, so the async path needs its own async constructors.

**Checked and correct**

- These citations are accurate:
  - `port.rs:43/91/135`
  - `approval.rs:42`, `environment.rs:47`, `hook.rs:139`, `event.rs:399`
  - `lib.rs:1805/2148/2189/2658/3080/3900/3918/4194/4392`
  - `harness-llm:370/494/539`
  - `harness-mcp:85/112/117/129/168/202`
  - `embedded.rs:82/163`
  - `operations.rs:196/394`, `local.rs:198`, `tools.rs:153`
  - catalogue `tests.rs:25/1196`, `verbs:269/343`, `flat:130/185`, `session.rs:499`
  - cli `lib.rs:2785/2865/3159/3265`
  - `boundary.rs:20/60/115`, app-server `lib.rs:589`
  - `llm-blocking:132`, `:87-98`
  - version 0.22.0, consumer counts 8/2/2/1, `metaharness-b10x/Cargo.toml:18`
- `harness-wire` has no tokio dependency.
- The counter test discriminates. Probe: `before=0 after_inline=0 after_spawn_blocking=50`.
- Writing items and spend per turn into `&mut Vec` / `&mut RunLedger` fits the borrow model (inferred).
- `tokio::spawn` of `run_async` works if the ports are owned inside the spawned task and the traits are `Send`.
- `BlockingLoop::on_runtime` can be built on `Handle::runtime_flavor`, as llm-blocking does.

I ran the probes (tokio 1.53.1, offline) under the session scratch directory and deleted them. I changed nothing in the worktree or the planning store.
