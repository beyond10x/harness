---
format: aep.planning-md/3
id: review-result:adversary-1105-u2-pass-1
kind: review-result
status: active
title: Adversary pass 1 on wave 2026-11-05 U2 (compaction counts and unknown opaque items)
relations:
- reviews: story:compacted-event-counts-dropped-reasoning
- reviews: story:compaction-keeps-unknown-opaque-items
revision: 1
---
```
unit: U2 (stories compacted-event-counts-dropped-reasoning, compaction-keeps-unknown-opaque-items), findings cover 685508e plus my uncommitted test additions
verdict: red
cases: executed 270→273, red 2
origin: introduced 3, pre-existing 0, undecided 0
wrote-outside-worktree: ~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1105/adv-u2/ (16 files and the mutants/ and mut/ directories, listed in part 6)
needs-coordinator: no
```

The unit's new fold keeps opaque items that are not reasoning, but it still counts their bytes when deciding a summary is worth a turn. So it can spend a model turn that folds nothing. If the kept item is big enough, it spends one before every request.

**1. Diff stat (one file, a test file)**
```
 crates/harness-loop/src/tests.rs | 175 +++++++++++++++++++++++++++++++++++++++
 1 file changed, 175 insertions(+)
```
I changed no implementation file. `cargo fmt -p b10x-harness-loop` changed only my own lines.

**2. Cases added** (in `crates/harness-loop/src/tests.rs`, plus a helper model `Adversary1105Model`)

- `adversary_1105_u2_a_summary_turn_is_not_spent_on_a_fold_that_can_remove_nothing`
  - Asserts: when the only item in the fold is a non-reasoning opaque item (34 kB), no summary turn is spent.
  - Now: **red**.
  - Red output when first run alone:
    ```
    assertion `left == right` failed: story:compaction-keeps-unknown-opaque-items: the only item in the fold is one the fold keeps, so a summary turn removes nothing and is spent for nothing; compactions as (elided, summarised, summary turn): [(0, 0, true)]
      left: 1
     right: 0
    ```
- `adversary_1105_u2_an_item_the_fold_keeps_does_not_buy_a_summary_turn_before_every_request`
  - Asserts: the same item over 4 conversation turns costs at most 1 summary turn.
  - Now: **red**.
  - Red output:
    ```
    story:compaction-keeps-unknown-opaque-items: 3 summary turns over 4 conversation turns, each folding nothing but the previous summary; compactions as (elided, summarised, summary turn): [(0, 0, true), (0, 1, true), (0, 1, true)]
    ```
  - Base probe: the same scenario on base `lib.rs` and `event.rs` in a scratch copy printed `BASE-PROBE summaries=1 compactions=[(0, 1, true)]`. So base spent one turn and that turn removed the item; the repeat comes from this unit.
- `adversary_1105_u2_a_kept_item_stands_right_after_the_summary_and_is_not_counted_as_folded`
  - Asserts: the next request is exactly `[task, summary, kept, c1, result c1]`, with `summarised_items` 2, `dropped_reasoning` 1 and `summary_turn` true.
  - Now: **green**. It is there to catch mutants m4, m5 and m6 below.

**3. Suite run** (after the cases existed)

`cargo test -p b10x-harness-loop --lib --locked`, exit 101:
```
test result: FAILED. 271 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.79s
```
The two failures are the two red cases above.

- `cargo fmt -p b10x-harness-loop --check`: exit 0 (after formatting my lines).
- `cargo clippy -p b10x-harness-loop --all-targets --locked -- -D warnings`: exit 0.
- Not run: `harness-cli` lib tests and clippy on `-p b10x-harness-cli`. I touched no CLI file, so they would only have cost build time.

**Mutation results** (scratch copy of HEAD with my tests; full loop lib suite each time)

| mutant | failing cases (beyond my 2 red ones where listed) | verdict |
|---|---|---|
| m1 `is_reasoning` always true | 7: the unit's 5 new cases and my case 3 | caught by the unit |
| m2 `is_reasoning` always false | 16, including every u3/u3b case from the last wave | caught by the unit; the old reasoning cases still catch their bugs |
| m3 fold drops kept items | `a_summary_fold_keeps_an_opaque_item_that_is_not_reasoning` and my case 3 | caught by the unit |
| m4 kept items placed after the tail | only my case 3 | **the unit's suite misses it** |
| m5 `summarised_items` also counts kept items | only my case 3 | **the unit's suite misses it** |
| m6 kept items placed before the summary | only my case 3 | **the unit's suite misses it** |

**Findings**

| file:line | what was measured | what reaches it | verdict | origin |
|---|---|---|---|---|
| crates/harness-loop/src/lib.rs:3413 | Whether a fold is worth a turn is decided on `measure(&folded)`, which includes the kept items. Red at tests.rs:9067 (1 turn spent, 0 items summarised) and tests.rs:9091 (3 summary turns over 4 conversation turns); base probe spent 1. Fix: measure only what the fold removes, and skip when that is nothing or under `SUMMARY_MIN_FOLD_BYTES`. | Needs a non-reasoning opaque item of about 8 KiB or more in the fold range. The shipped binary publishes only function tools, and I found no source that produces such an item. I built this state myself. | INFEASIBLE | introduced |
| crates/harness-loop/src/tests.rs:8937 | The unit's fold test asserts only `summarised > 0` and the item count, so m4, m5 and m6 pass every unit case. | Only the suite. | CONFIRMED | introduced |
| crates/harness-loop/src/lib.rs:1480 | `REASONING_ITEM_TYPES` copies the wire strings `RESPONSES_WIRE` and `MESSAGES_WIRE` from `harness-llm`, and no test ties the two together. The loop tests use the literal `"openai-responses"`. If `harness-llm` renamed a wire, reasoning would silently stop being dropped and the last wave's window problem would come back with every case green. Not built. | Nothing today; it needs a rename. | INFEASIBLE | introduced |

**4. Judgement residue** (covers 685508e): the third row above is the only one.

**5. Attacked and could not break**
- **`dropped_reasoning` count:**
  - It equals the warning's count on the fold-only, drop-only, both, window-fallback and failed-summary paths: one variable feeds both (lib.rs:3364 and lib.rs:3372).
  - It is 0 on the byte-only rule (lib.rs:1352).
  - A recorded line without the field reads as 0 (`serde(default)`).
- **Event JSON drift:** the field is always serialized, but nothing pins event JSON byte for byte. `contracts/`, `spec/` and `conformance/` have no `compacted` event. The app server drops `Compacted` (session.rs:415). Conformance reads only `summarised_items`.
- **`is_reasoning` against the pinned llm 0.17.0:**
  - Responses reasoning is the output item of type `reasoning`, encrypted or not (llm-responses stream.rs:806).
  - Messages reasoning is `thinking` or `redacted_thinking` (llm-messages codec.rs:765).
  - Neither wire produces a reasoning item without a `type`.
- **`assert_replayable`:** it now skips only non-reasoning opaque items, and only the unit's new cases build one. No existing assertion got weaker.
- **Not measured: whether providers accept the kept item.**
  - On Messages, llm-messages refuses to encode any opaque block that is not thinking (codec.rs:686-690). So a kept item there already fails the next request, whatever compaction does.
  - On Responses, whether the provider accepts a kept item whose reasoning was folded is a provider question I could not observe.

**6. Paths written outside the worktree** (all under `~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1105/adv-u2/`)
- `builds.tsv`
- Logs: `run1.log`, `run2.log`, `run-base.log`, `run-clippy.log`, `run-m1-always-true.log` … `run-m6-kept-before-summary.log` (6)
- Base copies: `base-append.rs`, `base-event.rs`, `base-lib.rs`, `base-tests.rs`
- Snapshots: `event.rs.pristine`, `lib.rs.pristine`, `tests.rs.orig`, `tests.rs.adv`
- `mutants/` (6 files)
- `mut/`: a source copy of the workspace; its `target/` was removed with `cargo clean --manifest-path`

**builds.tsv**: 14 cargo calls, 1.35 cargo minutes, 73.8 wall minutes; almost all of the wall time was waiting for the build slot. Peak `target/` was 479M in the tree and 419M in scratch (since cleaned). Free disk before the first build was 71G.

**Scope**

| path | status |
|---|---|
| crates/harness-loop/src/lib.rs (`is_reasoning`, `REASONING_ITEM_TYPES`) | confirmed |
| crates/harness-llm/src/lib.rs (`RESPONSES_WIRE`, `MESSAGES_WIRE`) | confirmed |
| crates/harness-loop/src/tests.rs | confirmed |
| `contracts/` and the CLI/app-server event tests | confirmed (nothing pins `compacted`) |

```findings
- file: crates/harness-loop/src/lib.rs
  line: 3413
  category: acceptance
  severity: warning
  verdict: INFEASIBLE
  origin: introduced
  message: summarise decides a fold is worth a turn on bytes that include the non-reasoning opaque items it now keeps, so a fold whose weight is a kept item spends a summary turn that removes nothing, and spends one again before every later request (3 over 4 turns against 1 at base).
- file: crates/harness-loop/src/tests.rs
  line: 8937
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: no unit case pins where a kept item lands or the exact summarised_items figure, so moving kept items after the tail or before the summary, or counting them as summarised, passes every unit case.
- file: crates/harness-loop/src/lib.rs
  line: 1480
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: introduced
  message: REASONING_ITEM_TYPES repeats harness-llm's wire identifiers with no check tying the two, so renaming a wire would silently stop compaction dropping reasoning while every loop test, which uses the literal, stays green.
```
