---
format: aep.planning-md/3
id: review-result:async-loop-scope-round-2
kind: review-result
status: active
title: Scope critic, round 2, async-native loop
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

I read all seven stories against the epic and found nothing the set fails to claim, nothing claimed twice and nothing beyond the epic.

Round 1's one blocker is fixed. The epic now says the 0.23.0 release is `story:blocking-facade-keeps-sync-callers`, and that story claims it. Its outcome says "the release says the port change is breaking". Its acceptance requires a `CHANGELOG.md` entry under a `Breaking` heading and workspace version `0.23.0`. Its scope lists `CHANGELOG.md` and `Cargo.toml`.

**What you read:** 9 artifacts. I ran `aep plan artifact show` on the epic (revision 2), on `review-result:async-loop-scope-round-1` and on the seven stories, and `aep plan artifact graph`. Promises extracted from the epic before reading any story: 17. They are two Outcome sentences, D1 to D6, five Done-When items and four Out-of-Scope constraints. All 17 trace to a story, except that the four Out-of-Scope constraints need no claimant and no story touches them.

| Promise | Story that claims it |
|---|---|
| Outcome 1: a tokio task awaits `AgentLoop::run`, with no thread and no nested runtime | `story:loop-runs-as-a-future` (spawned-task test) and the three migrate stories |
| Outcome 2: blocking entry point behaves as today | `story:loop-runs-as-a-future` and `story:blocking-facade-keeps-sync-callers` |
| D1, one async loop with a blocking facade | `story:loop-runs-as-a-future` |
| D2, boxed `Send` futures | `story:async-model-and-tool-ports` |
| D3, no tokio in `harness-wire` | `story:async-model-and-tool-ports` (`cargo tree` acceptance) |
| D4, synchronous ports adapted onto the blocking pool | `story:async-model-and-tool-ports` and `story:loop-runs-as-a-future` |
| D5, expand, migrate, contract | the stories' own step labels and `depends_on` edges |
| D6, `Operations` asynchronous and the confined backend awaited | `story:catalogue-awaits-its-operations` and `story:mcp-and-substrate-await-their-drivers` |
| Done When 1, the spawned-task await test | `story:loop-runs-as-a-future` |
| Done When 2, the existing suite passes | `story:loop-runs-as-a-future` and `story:blocking-facade-keeps-sync-callers` |
| Done When 3, the `grep` over llm, mcp and substrate returns nothing | the llm story and the mcp/substrate story, with the final guarantee in `story:blocking-facade-keeps-sync-callers` |
| Done When 4, `cargo xtask specification` | `story:binary-and-catalogue-on-one-runtime` |
| Done When 5, 0.23.0 and the `Breaking` heading | `story:blocking-facade-keeps-sync-callers` |

Reach beyond the parent: none. The `BlockingLoop::on_runtime` refusal and the drop-safety test both follow from the epic's blocking-facade promise and its `llm-blocking` evidence. Removing `std::thread::scope` follows from "the loop as a future". The rename to `Blocking*` follows from D5's "a last story renames". No story touches `llm-blocking`'s guard, `Toolset::invoke`, other repositories, `contracts/cli/` or the wire format.

Two items claiming one outcome: none. `story:catalogue-awaits-its-operations` and `story:mcp-and-substrate-await-their-drivers` both list `crates/harness-substrate/src/tools.rs`. Their outcomes differ: one makes the trait asynchronous, the other makes `ConfinedOperations` await the embedded backend.

**What you could not establish:**
- Out of my lane (parallel-safety): the two stories sharing `crates/harness-substrate/src/tools.rs` have an ordering edge, since the mcp/substrate story depends on the catalogue story. Whether the shared file also touches other stories' scopes is not my call.
- Out of my lane (acceptance and design): none beyond what round 1 already named.

```findings
[]
```
