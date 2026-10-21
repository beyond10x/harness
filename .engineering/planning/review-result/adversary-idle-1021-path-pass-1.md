---
format: aep.planning-md/3
id: review-result:adversary-idle-1021-path-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:one-spelling-of-a-path-in-every-workspace (wave idle-1021 at a9758870)
relations:
- reviews: story:one-spelling-of-a-path-in-every-workspace
revision: 1
---
idle-1021 adversary pass 1 over the merged wave (detached a9758870, tree harness-w1021-adv), path unit
(merge 040494e6): CONFIRMED, 1 red finding, fixed at 5fb1e839.

Added `crates/harness-substrate/tests/adversary_trailing_dot.rs` (2 cases, red before the fix, kept).

Findings:

1. `crates/harness-substrate/src/tools.rs` `without_dot_components` (minor, introduced) — the trailing `.`
   of `notes.txt/.` and `fresh/.` was dropped, so a confined workspace read `notes.txt` and created
   `fresh` while `LocalOperations` refused both, and `Split` read and wrote the same spelling
   differently. Fixed: a path ending in `/.` after a name is handed to substrate unchanged and refused;
   a path made only of `.` components is still `.`.

After the fix: `cargo test -p b10x-harness-substrate -p b10x-harness-workflow --locked` 121 passed,
0 failed; full `cargo xtask gate` at 5fb1e839 green, EXIT=0.
