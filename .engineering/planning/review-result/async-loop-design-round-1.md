---
format: aep.planning-md/3
id: review-result:async-loop-design-round-1
kind: review-result
status: active
title: Design critic, round 1, async-native loop
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

- story:async-model-and-tool-ports — it changes `ModelPort` and `ToolPort` but puts the loop out of scope, and the loop is their only caller (`model.turn` at `crates/harness-loop/src/lib.rs:2658` and `:3080`, `tools.call_batch` at `:4194`, `tools.call_within` at `:4392`). The call-site edits sit in `story:loop-runs-as-a-future`, so the workspace cannot build after this story alone. The story needs to take over the loop's call sites, or merge with `story:loop-runs-as-a-future`. — `.engineering/planning/story/async-model-and-tool-ports.md:36` (Out of Scope), `.engineering/planning/story/loop-runs-as-a-future.md:36` (Scope), `crates/harness-loop/src/lib.rs:2658`

- story:async-model-and-tool-ports — its acceptance "every implementor listed under Evidence compiles" claims files that later stories also own, and no body says who edits them.
  - The `ModelPort` implementor at `harness-llm:539` is also in `story:harness-llm-awaits-the-model`.
  - The `ToolPort` implementor at `harness-mcp:168` is also in `story:mcp-and-substrate-await-their-drivers`.
  - The `ToolPort` implementors in catalogue, cli, app-server and `conformance/boundary.rs` are also in `story:binary-and-catalogue-on-one-runtime`.
  - The body should say that these implementors are only wrapped in the D4 adapter here, and that the later stories remove the wrapper. — `.engineering/planning/story/async-model-and-tool-ports.md:14-19` (Evidence), `:25-26` (Acceptance); `story:binary-and-catalogue-on-one-runtime` Evidence, same list

- story:loop-runs-as-a-future — making `run` and `run_in` `async fn` breaks every synchronous caller, and the repair is split across two later stories with no edge to the right one.
  - Broken callers: `harness-cli/src/lib.rs:2785` and `:2865`, `harness-conformance/src/boundary.rs:115`, `harness-app-server/src/lib.rs:589`, and 131 call sites in `harness-loop/src/tests.rs`.
  - Its acceptance requires "the existing delegate tests pass", which needs either the blocking facade from `story:blocking-facade-keeps-sync-callers` or a rewrite of the tests.
  - That facade story depends on this one, so no edge can run the other way without a cycle.
  - `story:binary-and-catalogue-on-one-runtime` depends on this story but not on the facade, so the workspace is red from this story until the binary story lands.
  - The fix is to fold the facade into this story, or to keep a synchronous `run` beside the new async entry here and remove it in the facade story. That is an expand, migrate, contract split in which each item leaves the tree green. — `.engineering/planning/story/loop-runs-as-a-future.md:27-28` (Acceptance), `.engineering/planning/story/blocking-facade-keeps-sync-callers.md:25-26`, `aep plan artifact graph`

Scope and parallel-safety notes, out of my lane (they do not affect the verdict):
- The scope question is whether `HookPort`, `ApprovalPort`, `LoopSink` and `TurnEnvironmentProvider` in `harness-loop` are all covered.
- The parallel-safety question is whether stories 4, 5 and 6 can run at once, since all three edit `harness-conformance` and `harness-cli`.

What I read: 7 artifacts, with `aep plan artifact show` on each id, `relations`, `graph` and `validate` (valid; only unrelated review-outcome notices). I also grepped the code for the loop's port calls and for `run` and `run_in` callers.

What I could not establish:
- Whether `harness-loop/src/tests.rs` is meant to be rewritten to `#[tokio::test]` or to go through the facade. The bodies do not say.
- Whether the first finding is a blocker or a warning depends on whether the drafter intends one story to compile on its own. The bodies imply it does, so I recorded it as a blocker.

I walked all 14 declared edges in the store graph, including the two that leave the set (`serves vision:b10x-owns-its-loop` and `informed_by epic:embedded-by-a-consumer`). There is no cycle, and the shape is a fan-out rather than a chain. I traced about 9 acceptance reads to a producer in the set, and all are recorded by a `depends_on` edge. The one unrecorded read is the `tests.rs` read in the third finding.

```findings
- file: .engineering/planning/story/async-model-and-tool-ports.md
  line: 36
  category: design
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the story changes ModelPort and ToolPort but puts the loop out of scope, and the loop is their only caller (harness-loop lib.rs:2658, 3080, 4194, 4392), whose edits sit in story:loop-runs-as-a-future, so the workspace cannot build after this story alone; take the loop's call sites into this story or merge the two"
- file: .engineering/planning/story/async-model-and-tool-ports.md
  line: 25
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "acceptance 'every implementor listed under Evidence compiles' claims files that stories harness-llm-awaits-the-model, mcp-and-substrate-await-their-drivers and binary-and-catalogue-on-one-runtime also own, and no body says whether this story only wraps them in the D4 adapter and the later stories remove the wrapper"
- file: .engineering/planning/story/loop-runs-as-a-future.md
  line: 27
  category: design
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "making run and run_in async breaks every synchronous caller (harness-cli lib.rs:2785 and 2865, conformance boundary.rs:115, app-server lib.rs:589, 131 sites in tests.rs) and its acceptance 'existing delegate tests pass' needs the blocking facade that story:blocking-facade-keeps-sync-callers produces after it, with no edge possible without a cycle; fold the facade into this story or keep a sync run beside the async entry until the facade story contracts it"
```
