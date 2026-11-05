---
format: aep.planning-md/3
id: review-result:adversary-1104-u3-pass-2
kind: review-result
status: archived
title: Adversary pass 2 on wave 2026-11-04 U3 (after correction 1)
relations:
- reviews: story:summary-fold-keeps-reasoning-with-its-call
- reviews: story:compaction-shrinks-a-reasoning-heavy-conversation
revision: 3
transitions:
- {from: "active", to: "archived", at: "2026-11-05T01:03:45Z", actor: "human:timo", revision: 3}
---
unit: U3 (f0997a4 + correction d5751a7, merged at ef1c809), findings cover harness-1104-adv-u3b at ef1c809 plus my uncommitted tests.rs additions
verdict: CONFIRMED
cases: executed 260→264, red 1
origin: introduced 2, pre-existing 1, undecided 0
wrote-outside-worktree: 1 path (scratch dir ~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1104/adv-u3b)
needs-coordinator: whether the window regression (lib.rs:1479) holds U3 or goes to a backlog draft
```

The correction fixes both pass-1 findings it claimed. It also reopens one gap that f0997a4 had closed: when dropping reasoning cannot reach the target, the code now drops none, even where dropping would keep the next request inside the provider's window.

**1. Diff.** Only a test file changed:
```
 crates/harness-loop/src/tests.rs | 144 +++++++++++++++++++++++++++++++++++++++
 1 file changed, 144 insertions(+)
```

**Pass-1 findings the correction claims to fix**

- **tests.rs:3094 mutant: fixed.** I restored the old re-insertion of folded reasoning after the summary (M1). Three cases go red: pass 1's `adversary_u3_a_fold_whose_weight_is_text_keeps_no_folded_reasoning_after_the_summary`, the unit's P2 case `a_conversation_whose_weight_is_reasoning_reaches_its_target_in_one_compaction`, and my window case.
- **lib.rs:3221 boundary: fixed.** `adversary_u3_a_compaction_that_cannot_reach_its_target_keeps_the_reasoning_of_the_newest_call` is green at head. With f0997a4's drop-all behaviour restored (M2) it is red.
- **lib.rs:3239 and lib.rs:1458:** the correction did not change them, so I did not re-raise them.

**2. New cases**, all in `crates/harness-loop/src/tests.rs`:

| Case | What it asserts | State now |
|---|---|---|
| `adversary_u3b_a_target_met_exactly_by_one_drop_drops_one` | calls `drop_reasoning` directly; a target equal to the size after one drop takes exactly one item | green (kills M4) |
| `adversary_u3b_a_conversation_already_at_its_target_drops_nothing` | a conversation exactly at its target loses nothing | green (kills M3) |
| `adversary_u3b_the_reasoning_that_goes_is_the_oldest` | loop level, window 4,000, no summary turn; the oldest reasoning item goes and the newest stays | green (kills M5) |
| `adversary_u3b_reasoning_kept_over_an_unreachable_target_does_not_carry_the_next_request_past_the_window` | P2 parameters plus a 40 kB newest result and an 8 kB summary; the next request stays within the 40,000-token window | **red** |

The window case is red at head. This is its first run, of the new cases alone:
```
the request after compacting is an estimated 46062 tokens of the 40000 declared (23892 beyond its items), while it still carries 40124 bytes of reasoning whose drop would have left 36031 tokens
```
In that same run the exact-target case was also red, but only because I had the wrong index after the drop (`items[4]` instead of `items[3]`). Its main assertion, `(dropped, measure) == (1, target)`, passed. I fixed the index; it was a typo, not a finding.

**3. Suite run**, after the cases existed: `cargo test -p b10x-harness-loop --lib`
```
test tests::adversary_u3b_reasoning_kept_over_an_unreachable_target_does_not_carry_the_next_request_past_the_window ... FAILED
test result: FAILED. 263 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.82s
```
Exit 101. `cargo fmt -p b10x-harness-loop --check` exited 0. `cargo clippy -p b10x-harness-loop --all-targets --locked -- -D warnings` exited 0; it ran before a comment-only edit to my oldest-first case, and fmt passed again afterwards.

**Mutation results.** Run in a scratch copy, switched by an environment variable, never in the tree:

| Mutant | Failing cases |
|---|---|
| none | 1 failed: my window case |
| M1 (restore re-insertion) | 3: the unit's P2 case, pass 1's fold-text case, my window case |
| M2 (drop all when unreachable, i.e. f0997a4's behaviour) | 1: pass 1's newest-call case. **My window case is green under M2.** |
| M3 (early check `<=` becomes `<`, line 1462) | 2: only mine (at-target, window) |
| M4 (loop check `<=` becomes `<`, line 1474) | 2: only mine (exact-target, window) |
| M5 (newest reasoning first) | 4: the unit's `a_summary_fold_takes_the_reasoning_items_of_the_round_trips_it_folds`, plus my exact-target, oldest and window cases |
| M1+M9 (emulated base) | 8 |
| real base `lib.rs` at 2e37e2e (my two direct `drop_reasoning` cases removed, since the function does not exist there) | 255 passed, 7 failed; the window case reads `estimated 51078 tokens`, so it reproduces at the base |

**Findings**

| file:line | What was measured | What reaches it | Verdict | Origin |
|---|---|---|---|---|
| `crates/harness-loop/src/lib.rs:1479` | When no number of drops reaches the target, nothing is dropped. The next request is an estimated 46,062 tokens of a 40,000 window; dropping the 40,124 bytes of remaining reasoning would leave 36,031. f0997a4 (M2) passes; head and real base fail. | Constructed: the story's P2 parameters (three 20 kB reasoning items, window 40,000, reported 34,000) plus a 40 kB newest result and an 8 kB summary. The "over the window" figure is the loop's own 4-bytes-per-token estimate plus the overhead the reported 34,000 implies. | CONFIRMED | pre-existing |
| `crates/harness-loop/src/lib.rs:1462` | Mutant M3 (`<=` to `<`) survived the pre-existing suite; my at-target case now kills it | Exact equality of bytes only, so a coincidence in a real run | CONFIRMED | introduced |
| `crates/harness-loop/src/lib.rs:1474` | Mutant M4 (`<=` to `<`) survived the pre-existing suite; my exact-target case now kills it | Exact equality of bytes only | CONFIRMED | introduced |

On the window row: it reproduces at the base, but the regression comes from the correction itself, because f0997a4 kept that request under the window. The fix I would name: where no number of drops reaches the target, still drop the fewest oldest reasoning items that bring the conversation under the trigger level, computed in the same byte terms as `target`. Drop none only where even that is out of reach. I did not apply it.

**4. Attacked and could not break**
- `measure` against `measure_one`: `measure` is exactly their sum, with no per-item overhead.
- Reasoning interleaved with protected items, dropped oldest first: holds.
- The up-front count after elision and after a summary: it is computed on the current items and holds.
- Several compactions in one run: the earlier summary folds into the next one, and nothing broke.
- A failed summary followed by a drop: holds.
- Pass 1's "warns before every turn": with the target out of reach, compaction now returns `Unchanged` and does not warn.

**5. Paths written outside the worktree**, all under the scratch directory ~/.cache/scratch/fbf1604f-db7b-423e-bed5-b9a4dc96d66a/harness-1104/adv-u3b/:
- `mut/` (source copy; its `target/` was removed with `cargo clean`, 482.6 MiB)
- `lib.rs.mut`, `drop_reasoning_mut.rs`, `tests.rs.orig`, `timed.sh`, `run2.sh`, `run3.sh`
- `*.log`, `*.stamp`, `b1.wall`, `builds.tsv.done`, `builds.tsv`

**builds.tsv**
```
utc_start	cargo_min	wall_min	target	command	exit
2026-11-04T22:18:26Z	0.4	93.1	419M	cargo test -p b10x-harness-loop --lib adversary_u3b (new cases alone, worktree)	101
2026-11-05T00:39:27Z	0.1	1.4	419M	suite: cargo test -p b10x-harness-loop --lib (worktree)	101
2026-11-05T00:40:53Z	0.2	0.2	419M	mutbuild: cargo test -p b10x-harness-loop --lib --no-run (scratch mut)	0
2026-11-05T00:41:12Z	0.0	0.7	419M	fmt: cargo fmt -p b10x-harness-loop --check (worktree)	0
2026-11-05T00:41:52Z	0.2	0.2	479M	clippy: cargo clippy -p b10x-harness-loop --all-targets --locked -- -D warnings (worktree)	0
2026-11-05T00:42:40Z	0.1	1.6	419M	base: cargo test -p b10x-harness-loop --lib (scratch mut, base lib.rs)	101
2026-11-05T00:44:15Z	0.1	0.1	479M	suite2: cargo test -p b10x-harness-loop --lib (worktree)	101
2026-11-05T00:44:21Z	0.0	0.0	479M	fmt2: cargo fmt -p b10x-harness-loop --check (worktree)	0
2026-11-05T00:44:34Z	0.0	17.5		clean: cargo clean --manifest-path (scratch copy)	0
```
- **Totals:** 1.1 cargo minutes and about 115 wall minutes, mostly waiting for the build slot. No tell or stop line was reached.
- **Disk:** peak `target/` was 479M in the worktree and 419M in the scratch copy. `/` had 62G free before the first build and 68G at the resume.
- **Hold:** the chain I had queued when your hold arrived was killed by PID before any cargo process started.
- **Lease:** I took my own session lease on the tree and have released it. The test file is left uncommitted in the tree.

```findings
- file: crates/harness-loop/src/lib.rs
  line: 1479
  category: acceptance
  severity: warning
  verdict: CONFIRMED
  origin: pre-existing
  message: with the target out of reach drop_reasoning now drops nothing, so on the P2 parameters with a 40 kB newest result and an 8 kB summary the next request is an estimated 46062 tokens of a 40000 window where dropping the kept reasoning would leave 36031; f0997a4 passed this case and the correction re-broke it
- file: crates/harness-loop/src/lib.rs
  line: 1462
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the early return's `<=` mutated to `<` survived the suite the unit shipped; killed now by adversary_u3b_a_conversation_already_at_its_target_drops_nothing
- file: crates/harness-loop/src/lib.rs
  line: 1474
  category: mutant
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: the up-front count's `<=` mutated to `<` survived the suite the unit shipped; killed now by adversary_u3b_a_target_met_exactly_by_one_drop_drops_one
```
