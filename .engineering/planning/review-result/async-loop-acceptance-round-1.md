---
format: aep.planning-md/3
id: review-result:async-loop-acceptance-round-1
kind: review-result
status: active
title: Acceptance critic, round 1, async-native loop
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

- story:loop-runs-as-a-future — the drop-safety criterion has two outcomes, "they hold every turn completed before the drop. If this cannot hold, the rustdoc of `run_in` says which exit paths keep them and a test pins that", and either one passes, so the check cannot fail; the body must pick one required outcome — `.engineering/planning/story/loop-runs-as-a-future.md:34`
- story:blocking-facade-keeps-sync-callers — the first criterion, "either owns a runtime or borrows a caller's multi-thread runtime", is satisfied by every implementation and no test or command observes it, so it should go or be replaced by the observable behaviour of the three-thread test — `.engineering/planning/story/blocking-facade-keeps-sync-callers.md:28`
- story:mcp-and-substrate-await-their-drivers — the last criterion is conditional, "if a batch of reads now runs concurrently, the docs say so and a test shows two reads overlapping", so it passes when nobody checks concurrency; it should state whether the batch is concurrent after the work or still serial, and what the docs and test show in that case — `.engineering/planning/story/mcp-and-substrate-await-their-drivers.md:33`
- story:binary-and-catalogue-on-one-runtime — "The catalogue runs each synchronous `Toolset::invoke` on the blocking pool" names no test, grep or other observation, and today the catalogue calls `invoke` synchronously on the loop thread, so the criterion is not checkable; it needs a named test or command showing the call leaves the runtime worker — `.engineering/planning/story/binary-and-catalogue-on-one-runtime.md:34`
- story:binary-and-catalogue-on-one-runtime — the title and outcome promise "one runtime per process", but the criterion "`harness-cli` builds one runtime in `main` and awaits the loop; `harness-app-server` awaits it on its runtime" has no observable check (no grep count of runtime builders in `harness-cli`, no test) and joins two independent outcomes with "and"; it should name the check and split the two — `.engineering/planning/story/binary-and-catalogue-on-one-runtime.md:35`
- epic:async-native-loop — the done-when command `grep -rn 'new_current_thread\|BlockingModel' crates/*/src` runs over every crate's `src` and is expected to "find no runtime built per call … outside the blocking facade", but a grep cannot exclude the facade, which will hit if it owns a runtime. The sentence names only three crates while the command covers all of them, so it should name the crate paths to grep, matching story:harness-llm-awaits-the-model's form, or state where the facade lives and exclude that path — `.engineering/planning/epic/async-native-loop.md:86`

Read 7 artifacts (the six stories and the epic) in full with `aep plan artifact show <id>`, after `aep plan artifact kinds` and `aep plan artifact lifecycle story`. I checked the tree with `git grep` for `BlockingModel`, `current_thread`, `block_on`, `tokio` in `harness-wire` and `harness-loop`, and `llm-blocking` in `harness-llm`.

Could not establish:
- Whether `cargo tree -p b10x-harness-wire` (story:async-model-and-tool-ports) was run before the work. `crates/harness-wire/Cargo.toml` already lists no tokio, so that bullet guards against regression and is not a transition. I judged that acceptable.
- Out of lane: the slicing and ordering of the stories (design), whether the epic covers `harness-xtask` and `harness-app-server` (scope), and shared-file overlap between story 5 and stories 2–4 (parallel safety).
- Story acceptances are multi-bullet checklists, which is more than one statement. I did not raise this as a finding where every bullet is separately checkable and all must hold. Existing stories in this store use a paragraph, so the format differs from the store's convention.

```findings
- file: .engineering/planning/story/loop-runs-as-a-future.md
  line: 34
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the drop-safety criterion offers two outcomes, \"they hold every turn completed before the drop. If this cannot hold, the rustdoc of `run_in` says which exit paths keep them and a test pins that\", so either result passes and the check cannot fail"
- file: .engineering/planning/story/blocking-facade-keeps-sync-callers.md
  line: 28
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "\"either owns a runtime or borrows a caller's multi-thread runtime\" is satisfied by every implementation and no test or command observes it"
- file: .engineering/planning/story/mcp-and-substrate-await-their-drivers.md
  line: 33
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the last criterion is conditional (\"if a batch of reads now runs concurrently, the docs say so and a test shows two reads overlapping\"), so it passes when no concurrency is checked; it must state whether the batch is concurrent or serial afterwards and what the docs and test show"
- file: .engineering/planning/story/binary-and-catalogue-on-one-runtime.md
  line: 34
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "\"The catalogue runs each synchronous `Toolset::invoke` on the blocking pool\" names no test, grep or other observation of the before and after, so it cannot be checked"
- file: .engineering/planning/story/binary-and-catalogue-on-one-runtime.md
  line: 35
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the \"one runtime per process\" promise in the title and outcome has no observable check in the criterion \"`harness-cli` builds one runtime in `main` and awaits the loop; `harness-app-server` awaits it on its runtime\", which also joins two independent outcomes with \"and\""
- file: .engineering/planning/epic/async-native-loop.md
  line: 86
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the done-when grep over `crates/*/src` cannot exclude \"the blocking facade\", so it returns hits if the facade owns a runtime, and it covers more crates than the three the sentence names; it must name the crate paths or the facade's path"
```
