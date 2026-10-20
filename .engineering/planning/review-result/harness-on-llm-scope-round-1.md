---
format: aep.planning-md/3
id: review-result:harness-on-llm-scope-round-1
kind: review-result
status: active
title: Scope critic, round 1 (epic:harness-on-llm-and-tools set)
relations:
- reviews: epic:harness-on-llm-and-tools
- reviews: story:harness-builds-on-llm-0-7
- reviews: story:harness-consumes-tools-crates
- reviews: dependency-blocker:tools-has-no-release
revision: 1
---
approve

**What I read:** 4 artifacts (the epic and its 3 children) plus ADR 0097 and ADR 0096, via `aep plan artifact show` on each id and `aep plan artifact graph`. I also checked the spike's `llm_port.rs` (`git show 8989d138`) against `origin/main`.

I extracted 7 promises from the epic and ADR 0097 and traced all 7:
- Decision 1: harness stays at L1. It needs no work, and the epic's Outcome states it.
- Decision 2, wire crates go: `story:harness-builds-on-llm-0-7`.
- Decision 2, bridge rebased onto llm 0.7.0 (Rust 1.98): `story:harness-builds-on-llm-0-7`.
- Decision 2, compaction, retry, ledger and approvals stay: the same story, in its Out of scope and Safety fact.
- Decision 3, harness-tools and harness-toolchain move to `tools` and harness consumes them: `story:harness-consumes-tools-crates`.
- "Adopt `tools` once T1 ships": the same story, held by `dependency-blocker:tools-has-no-release`.
- Done-when, a release tag carries both stories: the drafter named it in `story:harness-builds-on-llm-0-7` Out of scope ("A release tag: `AGENTS.md` § Releases, after the wave closes"). I count that as a named omission, not a gap.

All three exclusions from the epic's Out of scope and ADR 0097 § Consequences are respected. No child moves a consumer's pin; the consumer table in story 1 is read-only information. No child touches metaharness's tool surface or llm's parity record. No item reaches beyond the parent. Re-enabling the Messages subscription-token path in story 1 item 3 undoes a spike-only refusal that `origin/main` did not have. I found no two items claiming the same outcome.

**What I could not establish:**
- The epic's Done-when names a release tag, but no item claims the tag and story 2 does not exclude it. I left it off the findings because story 1 names the release process.
- Out of my lane, for the acceptance and design critics: story 2's Outcome says `crates/harness-toolchain` no longer exists in the tree. Its own "Open until `tools` releases" section says whether `harness-toolchain` stays here is undecided.
- I did not verify the "six consumers" pin table in story 1 against sibling checkouts. The story says it is not verified either.

```findings
[]
```
