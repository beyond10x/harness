---
format: aep.planning-md/3
id: review-result:async-loop-acceptance-round-2
kind: review-result
status: active
title: Acceptance critic, round 2, async-native loop
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

- story:binary-and-catalogue-on-one-runtime — the app-server criterion, "`grep -rn 'runtime::Builder\|block_on' crates/harness-app-server/src` returns nothing; the app server awaits `run_in_async` on the runtime it is served on", reads the same before and after, because that grep already returns nothing at `origin/main`. The app server has no tokio dependency and no runtime today, so no named test or command shows it moved from `.run(` to `run_in_async`. Name an observable, for example `grep -n 'run_in_async' crates/harness-app-server/src/lib.rs` prints the call and `grep -n '\.run(' crates/harness-app-server/src/lib.rs` prints no loop call, or a test that drives a turn through the async path. — `.engineering/planning/story/binary-and-catalogue-on-one-runtime.md:66`
- story:harness-llm-awaits-the-model — the criterion "its turn, the renewal and the write at `:1441` are awaited on the caller's runtime" claims a write that is not part of `LlmPort`. `harness-llm/src/lib.rs:1441` is `Keychain::put`, a fixture helper inside `#[cfg(test)] mod tests` (`lib.rs:854-855`), and the test the story names covers a turn and one renewal, not that write. State that the fixture helper is converted, with the check, or drop it from the `LlmPort` criterion. — `.engineering/planning/story/harness-llm-awaits-the-model.md:32`
- story:blocking-facade-keeps-sync-callers — the outcome promises "A synchronous consumer keeps working through `run`, `run_in` and the `Blocking` adapters". After the round-1 rewrite no criterion observes it, because the three-thread test moved to `story:loop-runs-as-a-future`. The criteria check only the renames, two greps and the changelog. Add one criterion that a test implementing only `BlockingModelPort` and `BlockingToolPort` drives `run` to an outcome after the rename. — `.engineering/planning/story/blocking-facade-keeps-sync-callers.md:55`

Round 1 check: all six findings are fixed.
- Drop-safety now has one outcome: `loop-runs-as-a-future.md` writes items and spend per turn and tests a dropped future.
- The facade's own-a-runtime criterion is replaced by the three-thread test and the `on_runtime` refusal.
- The MCP and Substrate criterion now requires a barrier-of-2 batch.
- The catalogue criterion has a named counter test.
- The epic's done-when grep names three crate paths.
- The `harness-cli` runtime criterion is a countable grep. `crates/harness-cli/src` has no `runtime::Builder` today, so it passes as a transition.

Read 8 artifacts in full (`epic:async-native-loop` and the seven stories) with `aep plan artifact show <id>`. I also read `review-result:async-loop-acceptance-round-1` and ran `aep plan artifact kinds`. I checked the tree with `git grep` for `runtime::Builder|block_on|thread::scope` across the harness crates, and read `harness-llm/src/lib.rs`, `harness-app-server/src/lib.rs` and `harness-catalogue/src/catalogue.rs`.

Could not establish:
- `story:catalogue-awaits-its-operations` — the outcome says "every synchronous filesystem, search or process call", but the counter test covers one `LocalOperations` read, so process and search calls are not separately observed. I did not raise it because the criterion "`LocalOperations` runs each synchronous call through `tokio::task::spawn_blocking`" is checkable by reading. A FIFO fixture exists at `crates/harness-catalogue/src/local.rs:671`, so the 50 ms wait is feasible.
- Out of lane (design): `Catalogue::invoke_batch` uses `std::thread::scope` (`crates/harness-catalogue/src/catalogue.rs:682`). Neither the catalogue story nor the epic says what becomes of it once `Operations` is asynchronous, and `invoke_toolchain` drives the synchronous executor over `Operations`.
- Out of lane (scope): the epic's grep `BlockingModel` also substring-matches the renamed `BlockingModelPort`. I could not establish whether the three crates will mention that name after the contract story.

```findings
- file: .engineering/planning/story/binary-and-catalogue-on-one-runtime.md
  line: 66
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the app-server criterion, \"`grep -rn 'runtime::Builder\\|block_on' crates/harness-app-server/src` returns nothing; the app server awaits `run_in_async` on the runtime it is served on\", reads the same before and after because that grep already returns nothing at origin/main, and no named test or command shows the app server moved to `run_in_async`"
- file: .engineering/planning/story/harness-llm-awaits-the-model.md
  line: 32
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the criterion claims \"the write at `:1441`\" is part of LlmPort's awaited path, but lib.rs:1441 is the `Keychain::put` fixture inside `#[cfg(test)] mod tests`, and the named test covers only a turn and one renewal, so that clause is not observed"
- file: .engineering/planning/story/blocking-facade-keeps-sync-callers.md
  line: 55
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the outcome promises \"A synchronous consumer keeps working through `run`, `run_in` and the `Blocking` adapters\" but no acceptance criterion observes it once the three-thread test moved to story:loop-runs-as-a-future; the criteria check only renames, greps and the changelog"
```
