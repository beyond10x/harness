---
format: aep.planning-md/3
id: review-result:async-loop-design-round-2
kind: review-result
status: active
title: Design critic, round 2, async-native loop
relations:
- reviews: epic:async-native-loop
- reviews: story:async-model-and-tool-ports
- reviews: story:loop-runs-as-a-future
- reviews: story:harness-llm-awaits-the-model
- reviews: story:catalogue-awaits-its-operations
- reviews: story:mcp-and-substrate-await-their-drivers
- reviews: story:binary-and-catalogue-on-one-runtime
- reviews: story:blocking-facade-keeps-sync-callers
revision: 1
---
needs-revision

- story:catalogue-awaits-its-operations: it changes `Operations` to async in place and keeps `Flat` and `Verbs`'s synchronous `ToolPort` impls "until the facade story", but those impls call `Operations` synchronously through `Catalogue::invoke` and `invoke_batch`. No sentence says which runtime or `block_on` serves them in `harness-catalogue`, so the claim that the tree stays green and the existing catalogue tests pass is unsupported. The binary still uses the synchronous catalogue until `story:binary-and-catalogue-on-one-runtime`. The same sentence leaves `LocalOperations`'s `spawn_blocking` with no runtime to run on. The body must either keep a synchronous `Operations` beside the async one (expand) or name where the retained synchronous path gets its runtime, and say that the facade story removes it. The epic's `block_on` grep covers only `llm`, `mcp` and `substrate`, so a runtime in catalogue would pass it unnoticed. — `.engineering/planning/story/catalogue-awaits-its-operations.md:50-52`, `crates/harness-catalogue/src/operations.rs:196`, `crates/harness-catalogue/src/verbs.rs:269`, `crates/harness-catalogue/src/catalogue.rs:427`

Round 1's three findings are closed. The loop story now expands: `run` and `run_in` become a facade over `run_in_async`, and `story:async-model-and-tool-ports` adds async traits beside the synchronous ones. Each implementor is now wrapped in the adapter, not rewritten in place.

What I read: 8 artifacts (the epic, the seven stories and `review-result:async-loop-design-round-1`), with `aep plan artifact show` on each, plus `relations`, `graph` and `validate` (valid). I also read `crates/harness-catalogue/src/operations.rs`, `verbs.rs` and `Cargo.toml` in the worktree.

Walk and traces:
- I walked 22 declared edges for the set across the whole store graph, including those leaving it (`serves vision:b10x-owns-its-loop`, `informed_by epic:embedded-by-a-consumer`). There is no cycle. The shape is a fan-out that rejoins at the binary story, not a chain.
- I traced 11 acceptance reads to a producer inside the set, and `depends_on` records all 11.
- Two edges exist because two stories edit one file, and I did not ask for either to be removed. The `harness-substrate/src/tools.rs` overlap between the catalogue and MCP/Substrate stories is ordered by `depends_on`. The `harness-conformance` and `harness-cli` overlap between the binary story and the facade story is also ordered.

What I could not establish:
- **Out of my lane (scope or acceptance):** after the contract step, `run` and `run_in` take `BlockingModelPort`. `LlmPort` is async-only by then, because the synchronous implementation is deleted, and the `Blocking` adapters only go sync to async. The facade story's outcome says "A synchronous consumer keeps working", yet no body names an entry point for a synchronous caller holding a real `LlmPort`, such as `uilab`. Its acceptance does not show one either.
- **Not checked:** whether `Operations` has implementors outside this repository. That is a scope question.

```findings
- file: .engineering/planning/story/catalogue-awaits-its-operations.md
  line: 52
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "it changes Operations to async in place yet keeps Flat and Verbs's synchronous ToolPort impls, which call Operations synchronously (verbs.rs:269, catalogue.rs:427), and names no runtime or retained synchronous Operations for them, so green-at-every-step and 'existing catalogue tests pass' are unsupported; keep a synchronous Operations beside the async one or state where the retained path gets its runtime and that the facade story removes it"
```
