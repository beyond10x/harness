---
format: aep.planning-md/3
id: review-result:harness-on-llm-design-round-1
kind: review-result
status: active
title: Design critic, round 1 (epic:harness-on-llm-and-tools set)
relations:
- reviews: epic:harness-on-llm-and-tools
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
needs-revision
story:harness-consumes-tools-crates — harness-tools is built on harness-wire's tool vocabulary (`ToolPort`, `ToolSpec`, `Envelope`, `Risk`, `ToolOutcome`). harness-wire stays here and the sibling story leaves it untouched, and a `tools` crate cannot depend back on harness-wire. So the interface stays in one repository and its implementation moves to another, and neither the outcome, the scope nor "Open until tools releases" says who owns that vocabulary or the adapter — crates/harness-tools/Cargo.toml:17, crates/harness-tools/src/catalogue.rs:19, crates/harness-wire/src/port.rs:135, .engineering/planning/story/harness-consumes-tools-crates.md:77
story:harness-builds-on-llm-0-7 — four proposed stories (`story:oauth-token-renewal`, `story:responses-pin-from-live-bytes`, `story:messages-cache-breakpoint-live-pin`, `story:vllm-reasoning-events-unpinned`) keep acceptance on the crates this story deletes, and the graph holds no edge from this story to any of them. `aep plan artifact waves` already reports a `crates/harness-credential` collision with `story:oauth-token-renewal`. The body defers the move to the wave proposal; a `supersedes` edge from this story to each of the four would record it — .engineering/planning/story/harness-builds-on-llm-0-7.md:149-154, `aep plan artifact graph`

Read: 4 artifacts via `aep plan artifact show`, plus `relations`, `graph`, `validate` (valid; its only output is stale review-result notes, not mine), `waves`, the rubric, ADR 0097, `git show --stat 8989d138` and the harness-tools sources. I read the whole graph, 237 edges including those outside the set, and found no cycle. The set's edges are `blocks`, `decomposes` and `serves`, with no `depends_on`. I traced 2 acceptance reads to a producer: the `consumes` acceptance reads the `tools` tag the blocker covers (edge recorded), and the `builds` acceptance reads nothing the `consumes` story produces.
Could not establish: whether the two stories also need an ordering edge, since both edit `Cargo.toml`, `crates/harness-cli/Cargo.toml` and the documents. That is the parallel-safety lane, and the builds story already says the other is blocked and out of this wave. Out of my lane and not setting the verdict: the epic's "release tag" Done-when is owned by neither story.

```findings
- file: .engineering/planning/story/harness-consumes-tools-crates.md
  line: 77
  category: design
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "harness-tools is built on harness-wire's ToolPort, ToolSpec, Envelope, Risk and ToolOutcome, which stay here and are untouched by the sibling story, and a tools crate cannot depend back on harness-wire, so the outcome, scope and open questions name no owner for that vocabulary or the adapter and the seam falls between the two stories"
- file: .engineering/planning/story/harness-builds-on-llm-0-7.md
  line: 149
  category: design
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "four proposed stories still own acceptance on the four crates this story deletes and no supersedes edge records it, so two items claim the same surface until the wave proposal moves them"
```
