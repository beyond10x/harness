---
format: aep.planning-md/3
id: review-result:async-loop-scope-round-1
kind: review-result
status: active
title: Scope critic, round 1, async-native loop
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

- epic:async-native-loop — "The release is a minor bump of the `0.x` workspace version with the port change named in `CHANGELOG.md` as breaking." is promised and none of the six stories claims it. A grep of the six bodies for `CHANGELOG`, `minor bump` and `0.23` returns nothing, and the drafter's report names no omission. `story:binary-and-catalogue-on-one-runtime`, which depends on every other story, would most naturally take it. — `.engineering/planning/epic/async-native-loop.md:89-90`

**What you read:** 7 artifacts. I ran `aep plan artifact show` on the epic and the six stories, and `aep plan artifact graph`, which shows only these six stories decomposing the epic. I also read `.engineering/planning/story/changelog-in-the-same-change.md`, which is implemented and belongs to another epic, so it does not claim this release.

Promises extracted from the epic: 15. These are the 2 Outcome sentences, D1 to D4, the 5 Done-When items, and the 4 Out-of-Scope constraints. 14 trace to a story:
- Spawned-task await test: `story:loop-runs-as-a-future`.
- Blocking facade and its suite: `story:blocking-facade-keeps-sync-callers`.
- No per-call runtime in llm, mcp and substrate: `story:harness-llm-awaits-the-model` and `story:mcp-and-substrate-await-their-drivers`.
- Specification target: `story:binary-and-catalogue-on-one-runtime`.
- D1 to D4: the ports, loop and facade stories.
- Argv contract unchanged, and `Toolset::invoke` on the blocking pool: `story:binary-and-catalogue-on-one-runtime`.
- The `llm-blocking` guard and consumer migration are excluded, and no story touches them.

The release item is the one that does not trace.

I found no reach beyond the parent. The drop-safety test in the loop story and the refusal on a runtime worker in the facade story both follow from the epic's Outcome and its `llm-blocking` evidence. Each outcome is claimed once, except for the overlap noted below.

**What I could not establish:**
- **Out of my lane (parallel-safety/design):** `story:async-model-and-tool-ports` acceptance 4 ("Every implementor listed under Evidence compiles against the new traits") covers the same files as `story:binary-and-catalogue-on-one-runtime`, `story:harness-llm-awaits-the-model` and `story:mcp-and-substrate-await-their-drivers`. It also leaves the workspace's build order between the ports story and the loop story unstated. I did not let either set my verdict.

```findings
- file: .engineering/planning/epic/async-native-loop.md
  line: 89
  category: scope
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "\"The release is a minor bump of the `0.x` workspace version with the port change named in `CHANGELOG.md` as breaking.\" is promised and none of the six stories claims it; story:binary-and-catalogue-on-one-runtime would most naturally take it"
```
