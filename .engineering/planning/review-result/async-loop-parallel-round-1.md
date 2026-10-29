---
format: aep.planning-md/3
id: review-result:async-loop-parallel-round-1
kind: review-result
status: active
title: Parallel-safety critic, round 1, async-native loop
relations:
- reviews: epic:async-native-loop
- reviews: story:async-model-and-tool-ports
- reviews: story:loop-runs-as-a-future
- reviews: story:blocking-facade-keeps-sync-callers
- reviews: story:harness-llm-awaits-the-model
- reviews: story:mcp-and-substrate-await-their-drivers
- reviews: story:binary-and-catalogue-on-one-runtime
revision: 1
---
needs-revision

- story:async-model-and-tool-ports — carries its surface only in a `## Scope` body section and has no `scope:` front matter, so `aep plan artifact waves` lists it `unassessed` and never places it; record the cited paths and the inferred adapter home as `scope` entries (the front-matter form `story:approver-delegates-to-caller` uses) — .engineering/planning/story/async-model-and-tool-ports.md:40, `aep plan artifact waves` prints `unassessed: story:async-model-and-tool-ports`
- story:loop-runs-as-a-future — carries its surface only in a `## Scope` body section and has no `scope:` front matter, so `waves` lists it `unassessed` and never places it — .engineering/planning/story/loop-runs-as-a-future.md:40, `aep plan artifact waves` prints `unassessed: story:loop-runs-as-a-future`
- story:blocking-facade-keeps-sync-callers — carries its surface only in a `## Scope` body section and has no `scope:` front matter, so `waves` lists it `unassessed` and never places it — .engineering/planning/story/blocking-facade-keeps-sync-callers.md:35, `aep plan artifact waves` prints `unassessed: story:blocking-facade-keeps-sync-callers`
- story:harness-llm-awaits-the-model — carries its surface only in a `## Scope` body section and has no `scope:` front matter, so `waves` lists it `unassessed` and never places it — .engineering/planning/story/harness-llm-awaits-the-model.md:34, `aep plan artifact waves` prints `unassessed: story:harness-llm-awaits-the-model`
- story:mcp-and-substrate-await-their-drivers — carries its surface only in a `## Scope` body section and has no `scope:` front matter, so `waves` lists it `unassessed` and never places it — .engineering/planning/story/mcp-and-substrate-await-their-drivers.md:36, `aep plan artifact waves` prints `unassessed: story:mcp-and-substrate-await-their-drivers`
- story:binary-and-catalogue-on-one-runtime — carries its surface only in a `## Scope` body section and has no `scope:` front matter, so `waves` lists it `unassessed` and never places it — .engineering/planning/story/binary-and-catalogue-on-one-runtime.md:40, `aep plan artifact waves` prints `unassessed: story:binary-and-catalogue-on-one-runtime`
- story:blocking-facade-keeps-sync-callers — its facade dependency (tokio features on the root `Cargo.toml` `[workspace.dependencies] tokio`, and a new `harness-loop` dependency in `Cargo.lock`) lands where story:harness-llm-awaits-the-model also edits (it drops `llm-blocking` from `Cargo.lock`), and no `depends_on` edge orders the two, so they can share a wave (inferred; neither body names `Cargo.lock` or the root manifest) — .engineering/planning/story/blocking-facade-keeps-sync-callers.md:38, .engineering/planning/story/harness-llm-awaits-the-model.md:27, `Cargo.toml` line `tokio = { version = "1.53", features = ["rt", "time", "macros"] }`
- story:loop-runs-as-a-future — its scope names six `harness-loop` files, but its acceptance changes `ApprovalPort`, `HookPort`, `TurnEnvironmentProvider` and `LoopSink` and the run signature, whose implementors and callers sit in files no story in the set lists (`crates/harness-cli/src/approve.rs:25`, `caller.rs:53`, `hooks.rs:58`, `crates/harness-app-server/src/lib.rs:587`, `crates/harness-catalogue/tests/*.rs`, `crates/harness-cli/tests/*.rs`), so whichever implementor first hits the compile error edits files the plan never assigned, and the scope the wave planner would read is too narrow (inferred from the trait change; the files are cited from `git grep`) — .engineering/planning/story/loop-runs-as-a-future.md:40, `git grep -n "ApprovalPort\|HookPort\|AgentLoop" -- crates/harness-cli crates/harness-app-server crates/harness-catalogue`

What I read: 7 artifacts (the epic and the six stories) with `aep plan artifact show` on each, plus `aep plan artifact graph`, `relations`, `waves` and `validate` (validate: valid). I also ran `git grep` over `crates/` for implementors, callers, `block_on` and the crate manifests. Surfaces: all 6 stories cited a path in the body, and 3 of them (ports, blocking facade, mcp-and-substrate) also carry an inferred line; 0 were unplaceable by me, but 6 are `unassessed` to the `waves` verb because none has structured `scope`. I traced 8 acceptance reads to a producer in the set, and 7 of them are recorded by a direct `depends_on` edge. The 8th is the binary story's catalogue blocking-pool adapter (from the ports story), which is recorded only transitively through the loop, llm and mcp-and-substrate stories. The edges are ports to loop, llm and mcp-and-substrate; loop to blocking-facade; loop, llm and mcp-and-substrate to binary. The only unordered pairs are loop with llm and with mcp-and-substrate, blocking-facade with llm, mcp-and-substrate and binary, and llm with mcp-and-substrate. Their cited surfaces are disjoint apart from the lockfile and manifest finding above.

What I could not establish:
- Where the ports story's synchronous-to-async adapter lives: the body says only "outside `harness-wire`", so I cannot tell which story it collides with.
- Out of my lane, design or acceptance, and not affecting my verdict: `Operations` (`crates/harness-catalogue/src/operations.rs:196`) is a synchronous `Send + Sync` trait that the Substrate provider implements (`crates/harness-substrate/src/tools.rs`). The mcp-and-substrate story says the embedded backend's operations become async, and no story owns the trait or says whether it changes.
- Out of my lane: the loop story's "existing tests pass" is checked against about 157 call sites in `crates/harness-loop/src/tests.rs`, which have no blocking entry point to call until the blocking-facade story lands.

```findings
- file: .engineering/planning/story/async-model-and-tool-ports.md
  line: 40
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body names its surface only in a ## Scope section and has no scope front matter, so aep plan artifact waves lists it unassessed and never places it; record the cited paths and the inferred adapter home as scope entries"
- file: .engineering/planning/story/loop-runs-as-a-future.md
  line: 40
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body names its surface only in a ## Scope section and has no scope front matter, so aep plan artifact waves lists it unassessed and never places it"
- file: .engineering/planning/story/blocking-facade-keeps-sync-callers.md
  line: 35
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body names its surface only in a ## Scope section and has no scope front matter, so aep plan artifact waves lists it unassessed and never places it"
- file: .engineering/planning/story/harness-llm-awaits-the-model.md
  line: 34
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body names its surface only in a ## Scope section and has no scope front matter, so aep plan artifact waves lists it unassessed and never places it"
- file: .engineering/planning/story/mcp-and-substrate-await-their-drivers.md
  line: 36
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body names its surface only in a ## Scope section and has no scope front matter, so aep plan artifact waves lists it unassessed and never places it"
- file: .engineering/planning/story/binary-and-catalogue-on-one-runtime.md
  line: 40
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the body names its surface only in a ## Scope section and has no scope front matter, so aep plan artifact waves lists it unassessed and never places it"
- file: .engineering/planning/story/blocking-facade-keeps-sync-callers.md
  line: 38
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "inferred: the facade's tokio features on the root Cargo.toml and its new harness-loop dependency in Cargo.lock land where story:harness-llm-awaits-the-model also edits Cargo.lock (dropping llm-blocking), and no depends_on edge orders the two, so one wave can hold both; neither body names Cargo.lock or the root manifest"
- file: .engineering/planning/story/loop-runs-as-a-future.md
  line: 40
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "inferred: the scope names six harness-loop files but the acceptance changes ApprovalPort, HookPort, TurnEnvironmentProvider, LoopSink and the run signature, whose implementors and callers in harness-cli (approve.rs, caller.rs, hooks.rs, tests), harness-app-server src/lib.rs and harness-catalogue tests appear in no story's scope"
```
