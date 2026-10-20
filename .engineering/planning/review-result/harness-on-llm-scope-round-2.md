---
format: aep.planning-md/3
id: review-result:harness-on-llm-scope-round-2
kind: review-result
status: active
title: Scope critic, round 2 (epic:harness-on-llm-and-tools set)
relations:
- reviews: epic:harness-on-llm-and-tools
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
needs-revision
epic:harness-on-llm-and-tools — the Done-when promise "a release tag carries them, so a consumer has something to move to" is claimed by no item. Story 1 excludes it ("A release tag: `AGENTS.md` § Releases, after the wave closes") and story 2 is silent. Nothing records who cuts the tag, or that it is deliberately left out of the set. Story 1 would most naturally take it, or the epic's Done-when should say the tag is a release step outside the set. — .engineering/planning/epic/harness-on-llm-and-tools.md:63 (story:harness-builds-on-llm-0-7 body, Out of scope, line 198)
story:harness-consumes-tools-crates — the epic promises "`harness-tools` and `harness-toolchain` become `tools` crates" (ADR 0097 decision 3, quoted at epic line 27). The story's outcome and acceptance cover only `crates/harness-tools`, and `crates/harness-toolchain` "has moved with it or stays". The blocker says the story can be implemented with the toolchain staying. The story can therefore close and mark the epic done while half of decision 3 is unclaimed. No item or recorded decision takes the toolchain move if `tools` does not release it. — .engineering/planning/story/harness-consumes-tools-crates.md:92 (lines 113 and dependency-blocker:tools-has-no-release "Cleared when"; parent .engineering/planning/epic/harness-on-llm-and-tools.md:27)

Out of my lane, did not set the verdict: whether the Pin section and its acceptance are checkable (acceptance), and whether the adapter-location and seam text hold together (design).

```findings
[
  {
    "file": ".engineering/planning/epic/harness-on-llm-and-tools.md",
    "line": 63,
    "category": "scope",
    "severity": "blocker",
    "verdict": "needs-revision",
    "origin": "introduced",
    "message": "epic:harness-on-llm-and-tools — the Done-when promise \"a release tag carries them, so a consumer has something to move to\" is claimed by no item. Story 1 excludes it (\"A release tag: AGENTS.md § Releases, after the wave closes\", .engineering/planning/story/harness-builds-on-llm-0-7.md:198) and story 2 is silent. Story 1 would most naturally take it, or the epic's Done-when should say the tag is a release step outside the set."
  },
  {
    "file": ".engineering/planning/story/harness-consumes-tools-crates.md",
    "line": 92,
    "category": "scope",
    "severity": "blocker",
    "verdict": "needs-revision",
    "origin": "introduced",
    "message": "story:harness-consumes-tools-crates — the epic promises \"harness-tools and harness-toolchain become tools crates\" (ADR 0097 decision 3, .engineering/planning/epic/harness-on-llm-and-tools.md:27). The story's outcome and acceptance cover only crates/harness-tools; crates/harness-toolchain \"has moved with it or stays\" (lines 92 and 113), and dependency-blocker:tools-has-no-release \"Cleared when\" says the story can be implemented with the toolchain staying. Half of decision 3 is then unclaimed by any item and no decision records the omission."
  }
]
```
