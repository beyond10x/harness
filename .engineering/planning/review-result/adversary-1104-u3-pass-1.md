---
format: aep.planning-md/3
id: review-result:adversary-1104-u3-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave 2026-11-04 U3 (compaction keeps reasoning paired)
relations:
- reviews: story:summary-fold-keeps-reasoning-with-its-call
- reviews: story:compaction-shrinks-a-reasoning-heavy-conversation
revision: 1
---
Adversary pass 1 on U3 (unit f0997a4, merged at b9feab0), covering harness-1104-adv at 038f6d4 plus
the adversary's uncommitted test additions. Verdict NEEDS-CHANGE; cases executed 256→260, red 2;
origin introduced 5, pre-existing 0, undecided 0.

New cases in `crates/harness-loop/src/tests.rs`:

- `adversary_u3_a_compaction_that_drops_only_reasoning_says_so_in_its_figures` (red): one round
  trip with a 20 kB reasoning item, window 6,000; the fold is skipped and only reasoning is
  dropped. `the compacted event explains none of the 20062 bytes that went: elided 0, summarised 0,
  summary turn false`.
- `adversary_u3_a_compaction_that_cannot_reach_its_target_keeps_the_reasoning_of_the_newest_call`
  (red): the weight is a 100 kB task item nothing may touch, reasoning 2 kB each, window 30,000;
  each compaction strips the reasoning of the round trip that just happened and warns again.
  Passes on the base `lib.rs`, which compacted nothing here.
- `adversary_u3_a_fold_whose_weight_is_text_keeps_no_folded_reasoning_after_the_summary` (green):
  mutant-killer for the fold story.
- `adversary_u3_a_failed_summary_still_reaches_the_target_by_dropping_reasoning` (green).

Mutations (scratch copy only): M1 restores the old re-insertion: 257 passed, 3 failed; both unit
tests pass, the mint case is red (`left: [2, 3, 6]`). M2 makes `drop_reasoning` return 0: the
unit's P2 case is red (`one compaction left 20380 of 60630 bytes, above its 4630 byte target`).
Base `lib.rs`: 255 passed, 5 failed.

Suite with the cases: `test result: FAILED. 258 passed; 2 failed`; fmt exit 0 after `cargo fmt -p`;
clippy exit 0.

| file:line | what was measured | what reaches it | verdict | origin |
|---|---|---|---|---|
| `crates/harness-loop/src/tests.rs:3094` | M1 restores the re-insertion and both unit tests stay green | the story 1 acceptance test | NEEDS-CHANGE | introduced |
| `crates/harness-loop/src/lib.rs:3239` | a compaction that only drops reasoning emits `Compacted{elided 0, summarised 0, summary_turn false}` while 20,062 bytes went | a reasoning model with one heavy round trip near its window | NEEDS-CHANGE | introduced |
| `crates/harness-loop/src/lib.rs:3221` | `drop_reasoning` keeps going when the target cannot be met: it drops the newest reasoning and warns before every turn | any run whose weight is protected (task item or protected tail) | CONFIRMED | introduced |
| `crates/harness-loop/src/lib.rs:1452` | the story Outcome says opaque items go with their round trips; the code drops reasoning alone and keeps call and result; acceptance 3 holds | documentation only | CONFIRMED | introduced |
| `crates/harness-loop/src/lib.rs:1458` | the fold and `drop_reasoning` drop every `Item::Opaque`, including unknown output items llm preserves as `Opaque`, contrary to invariant 7 | nothing found: harness publishes only function tools | INFEASIBLE | introduced |

Attacked and could not break: a Responses call without its reasoning item (pinned llm 0.17.0 sends
`function_call` without an `id`, `request.rs:607-622`, `store:false`; not observed live); the
Messages route (harness sets no thinking control, `harness-llm/src/lib.rs:703-710`); a failed
summary followed by `drop_reasoning`; a target already met; the byte rule with no window (outside
story 2's acceptance); the stricter `assert_replayable`.

Cost: 1.2 cargo minutes, 12.4 wall minutes; peak target 479M (worktree) and 419M (scratch copy).

```findings
- file: crates/harness-loop/src/tests.rs
  line: 3094
  category: mutant
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: with the old re-insertion restored both unit tests stay green, because drop_reasoning removes the over-target re-inserted items in the same pass; only a fold under target exposes the defect
- file: crates/harness-loop/src/lib.rs
  line: 3239
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: a compaction that only drops reasoning emits a Compacted event with zero elided, zero summarised and no summary turn while 20062 bytes went, so the machine record cannot attribute the drop the story says is announced like elision
- file: crates/harness-loop/src/lib.rs
  line: 3221
  category: boundary
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: when the target cannot be met because the weight is protected, every compaction strips the newest round trip's reasoning and warns again before every turn, where the base compacted nothing
- file: crates/harness-loop/src/lib.rs
  line: 1452
  category: acceptance
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the story Outcome says opaque items go together with their round trips, while drop_reasoning drops them alone and keeps call and result, so story and code disagree
- file: crates/harness-loop/src/lib.rs
  line: 1458
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: the fold and drop_reasoning drop every Opaque item as reasoning, including unknown output items llm preserves as Opaque, contrary to invariant 7, but nothing found emits such an item
```
