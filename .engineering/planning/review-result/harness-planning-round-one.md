---
format: aep.planning-md/3
id: review-result:harness-planning-round-one
kind: review-result
status: active
title: Harness planning review, round one
relations:
- reviews: story:compaction-measured-live
- reviews: story:direct-provider-adapter
- reviews: story:responses-pin-from-live-bytes
- reviews: story:repo-local-profiles
- reviews: story:vllm-reasoning-events-unpinned
- reviews: initiative:record-matches-the-code
revision: 1
---
needs-revision

story:compaction-measured-live — The body still says no compaction figures have been measured live and needs to record the completed trigger/ratio observation separately from the outstanding summary-turn measurement — .engineering/planning/story/compaction-measured-live.md:26; STATUS.md:18

story:direct-provider-adapter — The acceptance retains the obsolete consumer-lifecycle check and needs to require the current TurnEnvironmentProvider binding and retained per-turn revision evidence — .engineering/planning/story/direct-provider-adapter.md:34; .engineering/planning/story/direct-provider-adapter.md:53; ROADMAP.md:201

story:responses-pin-from-live-bytes — The acceptance invokes the deleted Python provider checker and needs to name cargo xtask provider-contracts alongside the owning-crate contract test — .engineering/planning/story/responses-pin-from-live-bytes.md:38; crates/harness-xtask/src/main.rs:58

story:repo-local-profiles — The acceptance checks only profile digests and needs an observable repository-profile loading, trust, and precedence outcome after its prerequisite decisions are recorded — .engineering/planning/story/repo-local-profiles.md:44; .engineering/planning/story/repo-local-profiles.md:33

story:vllm-reasoning-events-unpinned — The acceptance promises silence for every response.reasoning_* event and needs to enumerate the measured admitted events while preserving warnings for unknown events — .engineering/planning/story/vllm-reasoning-events-unpinned.md:62; .engineering/planning/story/vllm-reasoning-events-unpinned.md:56; crates/harness-responses/src/lib.rs:435

initiative:record-matches-the-code — The drift-prevention completion clause has no owning child or decision blocker and needs a linked record for choosing a check or explicitly accepting the residual drift — .engineering/planning/initiative/record-matches-the-code.md:54; .engineering/planning/initiative/record-matches-the-code.md:60

Read: inventory and relations of the original 99 artifacts; 33 complete bodies covering every epic, initiative, open story, task, vision, specification, verification report and blocker; acceptance and review excerpts elsewhere. Used `aep plan artifact list`, `relations`, `validate`, and `lifecycle` for story, epic, initiative, task and decision-blocker, plus file reads and `rg`. Rechecked lifecycles after the protocol upgrade. The final validator sees 100 artifacts because the parent added the migration record during this pass.

Proposed corrections, not executed:

- Replace the five story bodies through `aep plan artifact body <id> --from -`, retaining dated historical evidence and making the current acceptance unambiguous.
- For `repo-local-profiles`, record its unresolved trust choices in a `decision-blocker` linked with `blocks:story:repo-local-profiles`; do not invent the answers.
- Create `decision-blocker:tracking-document-drift-policy` with `--relate blocks:initiative:record-matches-the-code --from -`, then update the initiative body to name that record. No lifecycle move is justified merely by this review.

Limits: This was a non-interactive, read-only review. No live provider, external consumer, deployment, repository controls or historical run was reverified. Implemented stories received inventory/acceptance sampling rather than a complete implementation audit. Historical `derived_from` parent links were treated as real parentage, not falsely reported as orphans solely because they are not `decomposes`. No finished-but-open epic was established. Validator warnings below are separate from the six substantive findings.

Validation rerun after revisions with `aep plan artifact validate --store .engineering/planning`; the CLI still printed the absolute checkout prefix, which is redacted below to leave repository-relative paths. All other output is unchanged, and this rerun does not alter the original review verdict or findings.

```text
105 file(s) in .engineering/planning: 105 artifact(s)
2 review(s) recorded no findings block:
  - review-result:harness-0-5-0-full-review states its findings as prose only — nothing can enumerate what it found, so the next review starts from nowhere
  - review-result:harness-public-site-sweep states its findings as prose only — nothing can enumerate what it found, so the next review starts from nowhere
2 review(s) have no recorded outcome:
  - review-result:harness-0-5-0-full-review was recorded 33 day(s) ago and nothing says what became of it — `aep plan artifact evidence <reviewed-id> --kind review_outcome --review review-result:harness-0-5-0-full-review --outcome no-op|fixed|escalated` is the record it is missing
  - review-result:harness-public-site-sweep was recorded 33 day(s) ago and nothing says what became of it — `aep plan artifact evidence <reviewed-id> --kind review_outcome --review review-result:harness-public-site-sweep --outcome no-op|fixed|escalated` is the record it is missing
valid
```

```findings
- file: .engineering/planning/story/compaction-measured-live.md
  line: 26
  category: drift
  severity: warning
  verdict: needs-revision
  origin: pre-existing
  message: The body still says no compaction figures have been measured live and needs to record the completed trigger/ratio observation separately from the outstanding summary-turn measurement
- file: .engineering/planning/story/direct-provider-adapter.md
  line: 34
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: pre-existing
  message: The acceptance retains the obsolete consumer-lifecycle check and needs to require the current TurnEnvironmentProvider binding and retained per-turn revision evidence
- file: .engineering/planning/story/responses-pin-from-live-bytes.md
  line: 38
  category: acceptance
  severity: warning
  verdict: needs-revision
  origin: pre-existing
  message: The acceptance invokes the deleted Python provider checker and needs to name cargo xtask provider-contracts alongside the owning-crate contract test
- file: .engineering/planning/story/repo-local-profiles.md
  line: 44
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: pre-existing
  message: The acceptance checks only profile digests and needs an observable repository-profile loading, trust, and precedence outcome after its prerequisite decisions are recorded
- file: .engineering/planning/story/vllm-reasoning-events-unpinned.md
  line: 62
  category: acceptance
  severity: blocker
  verdict: needs-revision
  origin: pre-existing
  message: The acceptance promises silence for every response.reasoning_* event and needs to enumerate the measured admitted events while preserving warnings for unknown events
- file: .engineering/planning/initiative/record-matches-the-code.md
  line: 54
  category: coverage
  severity: warning
  verdict: needs-revision
  origin: pre-existing
  message: The drift-prevention completion clause has no owning child or decision blocker and needs a linked record for choosing a check or explicitly accepting the residual drift
```

