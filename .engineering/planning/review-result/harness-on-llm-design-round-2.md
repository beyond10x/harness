---
format: aep.planning-md/3
id: review-result:harness-on-llm-design-round-2
kind: review-result
status: active
title: Design critic, round 2 (epic:harness-on-llm-and-tools set)
relations:
- reviews: epic:harness-on-llm-and-tools
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
needs-revision
story:harness-consumes-tools-crates — the new `## The seam` section settles only half of the tool layer's coupling to this repository: it keeps `harness-wire` out of the `tools` crates but ignores `harness-tools`' dependency on `harness-toolchain`. `harness-tools` re-exports and calls `harness_toolchain::ResolvedProvider`, `Registry` and `CommandPlan`. § Open and the blocker both let `harness-toolchain` "stay here" while `harness-tools` moves, which would make a `tools` crate depend back on a harness crate. That is the dependency the seam section forbids for `harness-wire`, and it forms a cross-repository cycle (harness → tools → harness-toolchain). The story must name which repository owns the toolchain types (`ResolvedProvider`, `Registry`, `CommandPlan`), or state that `harness-toolchain` moves with `harness-tools`. The blocker's "does not hold this blocker" clause is wrong unless that is answered first. — crates/harness-tools/Cargo.toml:16, crates/harness-tools/src/lib.rs:58, crates/harness-tools/src/catalogue.rs:75, .engineering/planning/story/harness-consumes-tools-crates.md:92-94, dependency-blocker:tools-has-no-release § Cleared when

Walked 14 edges touching the set and read the full graph, including edges outside the set. I found no cycle inside the graph. Its `depends_on` and `blocks` edges are acyclic.
- **Cross-repository cycle:** the finding above is a cycle the graph does not declare.
- **`depends_on` edge:** `story:harness-consumes-tools-crates` → `story:harness-builds-on-llm-0-7` records the shared-file ordering, with its reason written in both bodies, so it is not a serialising chain.
- **`supersedes` edges:** the four edges to the proposed stories close round 1's second finding.
- **Acceptance reads:** I traced 2 reads to producers: the `tools` tag (blocker edge recorded), and `consumes` reading what `builds` leaves in `Cargo.toml` and the documents (`depends_on` recorded).
- **Validator:** `validate` reports only stale review notes, which are not mine.
- **Out of my lane:** the epic's "release tag" Done-when is owned by neither story.

```findings
[{"file": ".engineering/planning/story/harness-consumes-tools-crates.md", "line": 92, "category": "design", "severity": "blocker", "verdict": "needs-revision", "origin": "introduced", "message": "The seam section keeps harness-wire out of the tools crates but not harness-toolchain. harness-tools depends on and re-exports harness_toolchain types (crates/harness-tools/Cargo.toml:16, crates/harness-tools/src/lib.rs:58, crates/harness-tools/src/catalogue.rs:75), yet § Open (this file, lines 92-94) and dependency-blocker:tools-has-no-release § Cleared when let harness-toolchain stay here while harness-tools moves. A tools crate would then depend back on a harness crate, a cross-repository cycle (harness -> tools -> harness-toolchain) and the same coupling the seam section forbids for harness-wire. Name the owner of the toolchain types (ResolvedProvider, Registry, CommandPlan), or state that harness-toolchain moves with harness-tools, and correct the blocker's 'does not hold this blocker' clause."}]
```
