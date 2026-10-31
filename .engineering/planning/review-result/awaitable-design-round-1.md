---
format: aep.planning-md/3
id: review-result:awaitable-design-round-1
kind: review-result
status: active
title: Design critic, round 1, awaitable loop
relations:
- reviews: epic:loop-awaitable-from-async-programs
- reviews: story:loop-task-awaits-a-run
- reviews: story:real-ports-run-under-an-awaited-loop
- reviews: story:embedding-guide-recommends-loop-task
revision: 1
---
needs-revision

- `story:real-ports-run-under-an-awaited-loop` — its acceptance constructs each `McpTools` "inside the async test body, on a runtime thread, to show construction is legal there too", but `McpTools::prepare` calls `runtime.block_on(...)` twice, which panics inside a runtime context. Move the construction line to a plain thread, or record that `prepare` is not legal there and say who constructs it. — `.engineering/planning/story/real-ports-run-under-an-awaited-loop.md:54`, `crates/harness-mcp/src/lib.rs:117`, `crates/harness-mcp/src/lib.rs:129`
- `epic:loop-awaitable-from-async-programs` — D1 hands the owned ports back to the async caller, but `McpTools` (`runtime: tokio::runtime::Runtime`) and `Embedded` own a tokio `Runtime`, and dropping one inside an async context panics. Probed on tokio 1.53.2: both `current_thread` and `multi_thread` print "Cannot drop a runtime in a context where blocking is not allowed". D1 needs a rule for where handed-back ports are dropped, and `story:real-ports-run-under-an-awaited-loop` needs an acceptance for it. — `.engineering/planning/epic/loop-awaitable-from-async-programs.md:47`, `crates/harness-substrate/src/embedded.rs:82`, `crates/harness-mcp/src/lib.rs:85`
- `story:loop-task-awaits-a-run` — D4's cancel-on-drop reaches a blocking model turn only through the token the port itself holds. `LlmPort` takes its `Cancel` at construction and checks it itself, and `AgentLoop::with_cancel` is the loop's token. The body does not say whether `LoopTask` takes the caller's `Cancel` and installs it on the loop, or creates its own. The drop acceptance uses a scripted model, so it would pass either way. — `.engineering/planning/story/loop-task-awaits-a-run.md:55`, `crates/harness-llm/src/lib.rs:529`, `crates/harness-llm/src/lib.rs:546`

D1 holds mechanically: `AgentLoop::new` takes `&'a mut dyn` ports (`lib.rs:2047`); a `Box<dyn … + Send>` can be reborrowed on the run thread. Shipped ports are `Send` (`LlmPort` has `assert_send`, `harness-llm/src/lib.rs:534`; `Catalogue`'s `Box<dyn Operations>` is `Send + Sync`, `operations.rs:196`); `harness-app-server` `BridgeTools`/`BridgeSink` use `Rc<RefCell>` (`session.rs:40`). D2, D3, D5 hold. No cycle in 453 edges. Not established: whether `b10x_mcp_client::Connection` is `Send`.

```findings
- file: .engineering/planning/story/real-ports-run-under-an-awaited-loop.md
  line: 54
  category: design
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance constructs each McpTools inside the async test body on a runtime thread to show construction is legal, but McpTools::prepare calls runtime.block_on twice (crates/harness-mcp/src/lib.rs:117,129), which panics inside a runtime context; the body must construct on a plain thread or record that construction is not legal there"
- file: .engineering/planning/epic/loop-awaitable-from-async-programs.md
  line: 47
  category: design
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "D1 hands owned ports back to the async caller, but McpTools and Embedded own a tokio Runtime whose drop panics in an async context (probed on tokio 1.53.2, both flavours); D1 must state where handed-back ports are dropped and story:real-ports-run-under-an-awaited-loop needs an acceptance for it"
- file: .engineering/planning/story/loop-task-awaits-a-run.md
  line: 55
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body does not say whether LoopTask takes the caller's Cancel or makes its own, though LlmPort checks only the token it was built with (crates/harness-llm/src/lib.rs:529,546), so cancel-on-drop may not reach a blocked turn; name the token's source and add an acceptance with a port that observes it"
```
