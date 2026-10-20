---
format: aep.planning-md/3
id: review-result:harness-on-llm-parallel-safety-round-2
kind: review-result
status: active
title: Parallel-safety critic, round 2 (epic:harness-on-llm-and-tools set)
relations:
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
needs-revision
story:harness-builds-on-llm-0-7 — its "Would collide with" line says seven paths "as the verb reports them", but the verb prints ten. The three it omits are `crates/harness-cli/src/lib.rs` (cited here, inferred there), `crates/harness-xtask/src/main.rs` (cited here, inferred there) and `spec/COVERAGE.md` (cited here, inferred there). Its Scope names all three itself (`src/lib.rs` 158 lines changed, `check_http_boundary` removed, `spec/COVERAGE.md:35-38`). The consumes story's list of ten matches the verb. The two bodies disagree, and the builds line cites the verb as its source. — .engineering/planning/story/harness-builds-on-llm-0-7.md:225-229, .engineering/planning/story/harness-consumes-tools-crates.md:147-150
story:harness-consumes-tools-crates — its surface is understated, so the verb's collision list is incomplete. Both sides name `ROADMAP.md`: builds cites it at `ROADMAP.md:91,99`; `ROADMAP.md:206-214` names `harness_tools::Operations`, `harness-tools` and `harness_tools::Verbs` (cited, `git grep`). The consumes story lists no `ROADMAP.md`, so the verb cannot print that collision. `README.md:229,259` also name `harness-tools`, and consumes cites only `README.md:474-475`. The `depends_on` edge orders the pair, but the plan does not say what it reaches here. — .engineering/planning/story/harness-consumes-tools-crates.md:143, ROADMAP.md:206-214
story:harness-consumes-tools-crates — the source files that call `harness_tools` or `harness_toolchain` are mostly unnamed in its scope. Unlisted, from `git grep -l`: `crates/harness-cli/src/approve.rs`, `metaharness.rs`, `workflow.rs`, `crates/harness-substrate/src/{embedded,tools,toolchain,tests}.rs`, `crates/harness-substrate/tests/{conformance,embedded_live,live}.rs` and `crates/harness-loop/src/tests.rs`. Only `lib.rs:115,122,207` and `xtask/main.rs:195-220` are named, plus Cargo.toml for substrate. None of these files is in builds' scope today, so no collision is hidden. But the surface is "named too narrowly to judge": a later edit to builds' scope could land on one of them unseen. The adapter location is also open, so some of the surface is unknown by the story's own account. — .engineering/planning/story/harness-consumes-tools-crates.md:143-146, `git grep -l 'harness_tools|harness_toolchain' -- crates`

Part 3 numbers.
- Items placed: 2 of 2 have a surface. Both are mixed, with cited and inferred entries (builds 20 cited and 2 inferred; consumes 7 cited and 8 inferred). None is unplaceable.
- Acceptance reads traced to a producer in the set: 1. Consumes' acceptance reads the `[workspace.dependencies]` pins and the workspace members, which builds rewrites. A `depends_on` edge records it. Waves puts builds in wave 1 and consumes in wave 2.

Out of my lane:
- `dependency-blocker:tools-has-no-release` is outside the set and blocks consumes. Whether builds can run alone while it is open is the operator's call.
- The "Open until `tools` releases" lines are an acceptance and design matter, not a concurrency one.

```findings
[
  {
    "file": ".engineering/planning/story/harness-builds-on-llm-0-7.md",
    "line": 225,
    "category": "parallel-safety",
    "severity": "blocker",
    "verdict": "needs-revision",
    "origin": "introduced",
    "message": "Its 'Would collide with' line says seven paths 'as the verb reports them', but `aep plan artifact waves` prints ten. Omitted are crates/harness-cli/src/lib.rs, crates/harness-xtask/src/main.rs and spec/COVERAGE.md (cited here, inferred in the other story), all of which its own Scope names. The consumes story lists the ten. The two bodies disagree, and this one cites the verb as its source. Remedy: list all ten paths; the depends_on edge already orders the pair. Cited: .engineering/planning/story/harness-consumes-tools-crates.md:147-150."
  },
  {
    "file": ".engineering/planning/story/harness-consumes-tools-crates.md",
    "line": 143,
    "category": "parallel-safety",
    "severity": "warning",
    "verdict": "needs-revision",
    "origin": "introduced",
    "message": "ROADMAP.md is named by builds (ROADMAP.md:91,99 at .engineering/planning/story/harness-builds-on-llm-0-7.md:217) and carries harness_tools references at ROADMAP.md:206-214 (cited, git grep), but consumes lists no ROADMAP.md, so the verb cannot print that collision. README.md:229,259 also name harness-tools and are unlisted (consumes cites only README.md:474-475). Add ROADMAP.md to consumes' scope or state it is untouched."
  },
  {
    "file": ".engineering/planning/story/harness-consumes-tools-crates.md",
    "line": 143,
    "category": "parallel-safety",
    "severity": "warning",
    "verdict": "needs-revision",
    "origin": "introduced",
    "message": "Surface named too narrowly to judge (cited by git grep -l 'harness_tools|harness_toolchain' -- crates, not by the body). Unlisted callers: crates/harness-cli/src/approve.rs, metaharness.rs, workflow.rs; crates/harness-substrate/src/{embedded,tools,toolchain,tests}.rs and tests/{conformance,embedded_live,live}.rs; crates/harness-loop/src/tests.rs. None is in builds' scope today, so no collision is hidden, but the pair cannot be fully assessed until the scope names them or says they are out; the adapter location is also open per the story's own Open section."
  }
]
```
