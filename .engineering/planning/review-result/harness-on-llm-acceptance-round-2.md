---
format: aep.planning-md/3
id: review-result:harness-on-llm-acceptance-round-2
kind: review-result
status: active
title: Acceptance critic, round 2 (epic:harness-on-llm-and-tools set)
relations:
- reviews: epic:harness-on-llm-and-tools
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
needs-revision
story:harness-builds-on-llm-0-7 — the acceptance clause "whose `crates/harness-cli/tests` carry no test ignored for an llm parity gap" cannot be read from the tree. The tree has other `#[ignore]` attributes, for example `crates/harness-substrate/tests/live.rs:24` (needs a daemon) and `crates/harness-cli/src/contract.rs:1431,1470`. Telling a parity-gap ignore from any other needs the author's judgement, so two checkers can disagree. State it as a command, for example no `#[ignore]` under `crates/harness-cli/tests`, or the three named tests running un-ignored. — .engineering/planning/story/harness-builds-on-llm-0-7.md:111-115
story:harness-consumes-tools-crates — the acceptance clause "whose `harness-loop` approval test that `AGENTS.md:138` describes (a `run` entry under `DenyAll` is refused) still passes" names no test. I found none by that description (grep of `crates/harness-loop` and `crates/harness-cli/tests`). `AGENTS.md:138` gives no test name, so the clause needs its author to say which test is meant. A `harness-loop` test is also unlikely to go through the new adapter, so it passes before and after the work and would not show that the adapter keeps `run` at a risk that triggers approval. Name the test, and add one that goes through the adapter's `ToolSpec` for `run`. — .engineering/planning/story/harness-consumes-tools-crates.md:95-101

Judged without a finding:
- epic:harness-on-llm-and-tools: "Done when" is a release tag that carries both stories. The latest tag, 0.9.1, carries neither, so the check differs before and after.
- dependency-blocker:tools-has-no-release: the state moves from `open` to `cleared`. Before, `tools` has no tag (`dabd034`). After, a tag exists and story § Open names it and the crate.

I read 4 of 4 ids in full. I also read the round-1 result, the lifecycles, `crates/harness-xtask/src/main.rs` (the gate includes `cli_contract::check`), `AGENTS.md:138` and `origin/spike/builds-on-llm-0.1.7` (its `Cargo.toml`, with the seven llm pins in `[workspace.dependencies]`).

I could not establish:
- Whether a `harness-loop` test for the `run`-under-`DenyAll` refusal exists under another name.
- The contents of `tools` at its first tag, so the story's § Open answers are unread.

Out of my lane, so they set no verdict:
- The epic needs a release tag that no artifact owns (story 1 puts it out of scope). That is a scope question.
- The seven-path overlap between the two stories is the parallel-safety critic's.

```findings
[
  {
    "file": ".engineering/planning/story/harness-builds-on-llm-0-7.md",
    "line": 111,
    "category": "acceptance",
    "severity": "warning",
    "verdict": "needs-revision",
    "origin": "introduced",
    "message": "Acceptance clause 'carry no test ignored for an llm parity gap' needs judgement to apply: other #[ignore] attributes exist (harness-substrate/tests/live.rs:24, harness-cli/src/contract.rs:1431,1470), so it is not mechanically checkable. Restate it as a command or as the three named tests running un-ignored."
  },
  {
    "file": ".engineering/planning/story/harness-consumes-tools-crates.md",
    "line": 95,
    "category": "acceptance",
    "severity": "warning",
    "verdict": "needs-revision",
    "origin": "introduced",
    "message": "Acceptance clause 'harness-loop approval test that AGENTS.md:138 describes ... still passes' names no test (AGENTS.md:138 gives no test name and a grep found none by that description). A harness-loop test is unlikely to go through the new adapter, so it passes before and after and does not show the adapter keeps run at a risk that triggers approval. Name the test, and add one that goes through the adapter's ToolSpec for run."
  }
]
```
