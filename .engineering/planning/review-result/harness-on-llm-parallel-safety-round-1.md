---
format: aep.planning-md/3
id: review-result:harness-on-llm-parallel-safety-round-1
kind: review-result
status: active
title: Parallel-safety critic, round 1 (epic:harness-on-llm-and-tools set)
relations:
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
needs-revision
story:harness-consumes-tools-crates — it shares seven files with story:harness-builds-on-llm-0-7 (Cargo.toml and crates/harness-cli/Cargo.toml cited; AGENTS.md, CHANGELOG.md, Cargo.lock, README.md and STATUS.md inferred), and its body has no scope or collision section, never names the other story, and has no depends_on edge to it; the only ordering it states is the tools-release blocker — .engineering/planning/story/harness-consumes-tools-crates.md:38-62 (no Scope heading; `aep plan artifact waves --kind story --status draft --format json` collisions)
story:harness-consumes-tools-crates — its scope lists only manifests, but harness-cli/src/lib.rs (harness_tools:: at lines 115, 122, 207) and harness-xtask/src/main.rs (harness_toolchain:: at lines 195-220) call the crates it replaces, so the verb's list misses collisions on two source files the other story edits, plus README.md:474-475 and spec/COVERAGE.md:44, which name harness-tools (inferred: whether the replacement keeps the crate names is open until tools releases) — .engineering/planning/story/harness-consumes-tools-crates.md:11-37, git grep -n "harness_tools\|harness_toolchain" crates/harness-cli/src/lib.rs crates/harness-xtask/src/main.rs
story:harness-builds-on-llm-0-7 — its "Would collide with" line lists six files and omits Cargo.lock, which the verb reports as the seventh collision; it also calls all six cited while the verb marks five of the seven inferred (they rest on the other story's inferred entries); the line also says "it is blocked and not in this wave", which no depends_on edge records — .engineering/planning/story/harness-builds-on-llm-0-7.md:214

Out of lane: the other story's acceptance and the split itself. Counts: 1 of 2 stories fully cited, 1 with six inferred entries, 0 unplaceable, 0 acceptance reads traced to a producer. I ran the CLI's `show` and `waves` verbs, `graph` and `relations`, and `git grep`.

```findings
- file: .engineering/planning/story/harness-consumes-tools-crates.md
  line: 11
  category: parallel-safety
  severity: blocker
  verdict: needs-revision
  origin: introduced
  message: "it shares seven files with story:harness-builds-on-llm-0-7 (Cargo.toml and crates/harness-cli/Cargo.toml cited; AGENTS.md, CHANGELOG.md, Cargo.lock, README.md and STATUS.md inferred); its body has no scope or collision section, never names the other story, and no depends_on edge orders them, so add an ordering edge that records the shared files as its reason or split the surface"
- file: .engineering/planning/story/harness-consumes-tools-crates.md
  line: 11
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "its scope lists only manifests, but crates/harness-cli/src/lib.rs and crates/harness-xtask/src/main.rs call harness_tools and harness_toolchain and the other story edits both, as it does README.md and spec/COVERAGE.md; inferred, because whether the replacement crates keep their names is open until tools releases; state the source files in scope or say they are untouched"
- file: .engineering/planning/story/harness-builds-on-llm-0-7.md
  line: 214
  category: parallel-safety
  severity: warning
  verdict: needs-revision
  origin: introduced
  message: "its Would collide with line lists six files and omits Cargo.lock, which the verb reports as the seventh; it also calls them all cited while the verb marks five of the seven inferred, and it records the ordering only as prose, not as a depends_on edge"
```
