---
format: aep.planning-md/3
id: review-result:async-loop-parallel-round-2
kind: review-result
status: active
title: Parallel-safety critic, round 2, async-native loop
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
approve

I read the seven revised stories and the epic. They are disjoint except where a `depends_on` chain orders them, so I found nothing that changes the draft.

**What I read.** I read 8 artifacts (the epic and the seven stories) with `aep plan artifact show` on each, plus the round-1 record `review-result:async-loop-parallel-round-1`. I also ran `aep plan artifact waves`, `validate` (valid) and `graph`. I checked the tree with `git grep` for port implementors, `thread::scope` and `AgentLoop` callers, and with `cargo tree --offline -e features -i tokio` for the harness-loop, harness-mcp and harness-substrate dependency trees.

**Surfaces.**
- Cited: 7 of 7 stories. Four of them also carry inferred entries (ports, catalogue, binary, facade).
- Inferred only: 0.
- Unplaceable: 0.
- `waves` places all seven. The round-1 `unassessed` defect is gone because each story now has `scope` front matter.

**Acceptance reads.** I traced 12 reads to a producer in the set, and 10 are recorded by a direct `depends_on` edge.
- Direct: llm, loop, catalogue and mcp each read from ports. mcp also reads from catalogue (the async `Operations`). binary reads from loop, llm, mcp and catalogue. facade reads from binary.
- Transitive: facade's grep acceptance over `harness-llm`, `harness-mcp` and `harness-substrate` reads what the llm and mcp stories produce. Those two edges are carried through binary.

**Collisions.** Every in-set pair that `waves` reports as colliding is ordered by a `depends_on` chain, so none can share a wave. I checked the lockfile and manifest collision that round 1 raised: the facade's two lockfile edits are now behind binary. The only pair that shares a file and is not obviously a chain is `crates/harness-substrate/src/tools.rs` (catalogue and mcp-and-substrate), and a direct edge orders it.

**What I could not establish.**
- Out of my lane, acceptance, and it does not set my verdict: `story:loop-runs-as-a-future` says "No dependency changes, so no manifest or lockfile" (line 40 of its body). Its `#[tokio::main]` test and its runtime-worker test need tokio's `rt-multi-thread` feature.
  - The root `Cargo.toml:59` enables only `rt`, `time` and `macros`.
  - `cargo tree -p b10x-harness-loop` shows no tokio in that crate's tree today, so nothing else enables the feature.
  - Only `story:async-model-and-tool-ports` and `story:blocking-facade-keeps-sync-callers` list the manifest, and both are ordered with the loop story. The claim is therefore a scope understatement and not a collision, but the acceptance critic should check it.
- Not a collision, and no peer to collide with: `crates/harness-loop/src/parallel.rs` and `delegate.rs` hold loop-port implementors (`parallel.rs:93`, `:111`, `:149`) that the loop and facade stories must change, but neither lists them. No other story in the set touches those files.
- Collisions that `waves` reports against stories outside this set (for example `story:api-key-from-the-secrets-store` on `crates/harness-cli/src/lib.rs`) are outside what I was given, so I did not judge them.

```findings
[]
```
