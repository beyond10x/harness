---
format: aep.planning-md/3
id: review-result:harness-on-llm-acceptance-round-1
kind: review-result
status: active
title: Acceptance critic, round 1 (epic:harness-on-llm-and-tools set)
relations:
- reviews: epic:harness-on-llm-and-tools
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
needs-revision
story:harness-builds-on-llm-0-7 — the acceptance accepts `0.8.0 once tagged`, so the same tree passes or fails depending on when it is checked, and decision 1 binds the move to 0.8.0 only if it is tagged before the unit's gate run — .engineering/planning/story/harness-builds-on-llm-0-7.md:112
story:harness-consumes-tools-crates — the acceptance clause "still read-only and still meets the approval gate" names no command, test or gate step that shows it, so it is a claim rather than an observable — .engineering/planning/story/harness-consumes-tools-crates.md:70-75
story:harness-consumes-tools-crates — the acceptance requires `crates/harness-toolchain` gone from the members and "the same tool names the CLI contract pins", which settles two questions the body lists as still open — .engineering/planning/story/harness-consumes-tools-crates.md:79-83
dependency-blocker:tools-has-no-release — "cleared when" requires a tag with replacements for both `harness-tools` and `harness-toolchain`, so it never clears if `harness-toolchain` stays in this repository, which the story leaves open, and it gives no way to tell that a crate is a "replacement" — .engineering/planning/dependency-blocker/tools-has-no-release.md:19

What I read: 4 of 4 ids (epic:harness-on-llm-and-tools, story:harness-builds-on-llm-0-7, story:harness-consumes-tools-crates, dependency-blocker:tools-has-no-release), each in full with `aep plan artifact show`, plus `aep plan artifact kinds` and `lifecycle` for the blocker and epic. I checked the tree for ignored tests, the `xtask gate` verb and the existing tags.

Judged without a finding:
- epic:harness-on-llm-and-tools: "Done when" is a tag that carries both stories, and the latest existing tag, 0.9.1, carries neither, so the check differs before and after.
- story:harness-builds-on-llm-0-7: the four crates are workspace members at `3169042f` and `cargo xtask gate` exists, so the check differs before and after.

What I could not establish:
- Whether `cargo xtask gate` already checks the read-only toolset and the approval gate. I did not read gate internals past the `gate` entry point.
- Out of my lane, so it set no verdict: story 1 puts the release tag out of scope, and the epic's "Done when" needs one, so no artifact owns cutting it. That is a scope and coverage question.

```findings
- file: .engineering/planning/story/harness-builds-on-llm-0-7.md
  line: 112
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "the acceptance accepts `0.8.0 once tagged`, so the same tree passes or fails depending on when it is checked, and decision 1 binds the move to 0.8.0 only if it is tagged before the unit's gate run"
- file: .engineering/planning/story/harness-consumes-tools-crates.md
  line: 70
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance clause 'still read-only and still meets the approval gate' names no command, test or gate step that shows it, so it is a claim rather than an observable"
- file: .engineering/planning/story/harness-consumes-tools-crates.md
  line: 79
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "the acceptance requires `crates/harness-toolchain` gone from the members and 'the same tool names the CLI contract pins', which settles two questions the body lists as still open"
- file: .engineering/planning/dependency-blocker/tools-has-no-release.md
  line: 19
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "'cleared when' requires a tag with replacements for both `harness-tools` and `harness-toolchain`, so it never clears if `harness-toolchain` stays in this repository, which the story leaves open, and it gives no way to tell that a crate is a 'replacement'"
```
