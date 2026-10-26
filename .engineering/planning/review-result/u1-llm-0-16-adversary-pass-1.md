---
format: aep.planning-md/3
id: review-result:u1-llm-0-16-adversary-pass-1
kind: review-result
status: active
title: U1 llm 0.16.0 adversary pass 1
summary: red 2 (README and STATUS pin drift, introduced), 1 undecided note; code unbroken
relations:
- reviews: story:harness-builds-on-llm-0-16
revision: 1
---
unit: story:harness-builds-on-llm-0-16 (tree at 23a6eb4 plus 2 untracked adversary test files)
verdict: red
cases: executed 1096→1106, red 2
origin: introduced 2 / pre-existing 0 / undecided 1
wrote-outside-worktree: 2 paths (part 6)
needs-coordinator: yes. README.md and STATUS.md are yours to fix (findings 1 and 2).

The code holds up. Both red cases are docs that still name the old pin.

**2. Cases added (each run alone before the suite)**

| File:line | Asserts | Now |
|---|---|---|
| `crates/harness-llm/tests/adversary_u1_llm_0_16.rs:69` | If the version README names is already a tag, README's llm tag equals that tag's pin | red |
| same file `:101` | STATUS.md names no current llm pin other than the one Cargo.toml holds | red |
| same file `:265` | A cost only on `message_start.message.usage` gives 1 warning, shown before the text, and the same outcome as a turn without it | green |
| same file `:287` | A cost on both usage objects gives 2 warnings, in stream order | green |
| same file `:300` | Costs of -0.5, 0, a string, `true`, an array and a nested object are carried as compact JSON; `null` gives no warning | green |
| same file `:325` | A cost next to an unknown sibling key (`is_byok`) is still refused (Unsupported, not retriable) after the text was shown | green |
| same file `:351` | A cost over 1 KiB is refused as TooLarge after the text | green |
| `crates/harness-cli/tests/adversary_u1_llm_0_16_cli.rs:165` | Under `--json`: exactly one usage-extension warning line, one usage event, and no cost event without a rate card | green |
| same file `:189` | With a rate card, the run's cost is `[27]` micro-USD (tokens times rates), not 12300 (the route's figure) | green |
| same file `:208` | Without `--json`: exactly one stderr line `warning [usage-extension] message_delta.usage.cost: 0.0123` | green |

Red output (`cargo test -p b10x-harness-llm --locked --test adversary_u1_llm_0_16`):

```
STATUS.md names an llm pin Cargo.toml does not hold (0.16.0): ["STATUS.md:13: llm `0.15.0`", "STATUS.md:17: llm tag `0.15.0`", "STATUS.md:27: llm tag `0.15.0`"]
assertion `left == right` failed: README.md says release `0.20.0` pins llm tag `0.16.0`; the tag `0.20.0` pins `0.15.0`
test result: FAILED. 5 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

Suite: `cargo test --workspace --locked --no-fail-fast`: EXIT=101, 1104 passed, 2 failed, 6 ignored;
the only failures are the 2 cases above.

**Attacked and could not break:** the hand-minimised lockfile (`cargo metadata --locked --offline`
exit 0; `cargo update --workspace --offline` on a scratch copy locks 0 packages, byte-identical);
the warning reaches the loop, the `--json` stream and stderr with no duplicates; cost placement and
shapes; the route's cost never becomes the run's cost; llm-messages turns on serde_json's
`float_roundtrip` for the whole build and nothing broke; a mutant dropping the warning in
`from_llm_event` fails the unit's own test.

```findings
- file: README.md
  line: 25
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: README dates the llm 0.16.0 pin to release 0.20.0, whose tag pins llm 0.15.0, and the existing README pin check cannot see an already-tagged version
- file: STATUS.md
  line: 13
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: STATUS.md rows at lines 13, 17 and 27 still state llm 0.15.0 as the current pin after the move to 0.16.0
- file: crates/harness-llm/tests/adversary_u1_llm_0_16.rs
  line: 325
  category: judgement
  severity: note
  verdict: INFEASIBLE
  origin: undecided
  message: llm names only the first unknown usage key in sorted order, so the live refusal does not rule out a sibling key after cost that would still end the turn after the answer, and no live capture shows the route's real usage shape
```
