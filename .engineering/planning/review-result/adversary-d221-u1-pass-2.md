---
format: aep.planning-md/3
id: review-result:adversary-d221-u1-pass-2
kind: review-result
status: active
title: Adversary pass 2 on the refusal escape (wave d221 U1)
relations:
- reviews: story:substrate-refusal-display-escapes-daemon-bytes
revision: 1
---
Adversary pass 2 (aep:adversary, opus) on `story:substrate-refusal-display-escapes-daemon-bytes`, correction aec6a5d, cases committed as 73c83db on `adv/d221-u1` (on top of ee50ac1).

```
unit: story:substrate-refusal-display-escapes-daemon-bytes correction aec6a5d, covering adv/d221-u1 at 73c83db (cases on top of ee50ac1)
verdict: NEEDS-CHANGE
cases: executed 365→368 (substrate suite 120→122 + CLI --lib and both adversary targets 245→246), red 3
origin: introduced 2 / pre-existing 1 / undecided 0
needs-coordinator: whether the withheld-note leak is fixed in this unit or the CHANGELOG claim is narrowed
```
1. Diff stat (ee50ac1..HEAD): `crates/harness-cli/tests/adversary_d221_u1_p2.rs` (+84), `crates/harness-substrate/tests/adversary_d221_u1_p2.rs` (+26).
2. Cases:
   - CLI `a_hostile_fact_in_a_withheld_note_does_not_reach_stderr_raw` red: raw ['\u{9b}', '\u{7f}', '\u{202e}'] in the `run` withheld note, `test result: FAILED. 0 passed; 1 failed`.
   - Substrate `a_raw_control_character_and_its_spelled_out_escape_display_differently` and `a_raw_line_break_and_a_spelled_out_one_display_differently` red, `test result: FAILED. 0 passed; 2 failed`.
3. Suites exit 101 with only these three cases failing.
4. Findings:
   - Withheld-note leak. `crates/harness-substrate/src/withheld.rs:87` (`describe`) quotes a daemon fact value with serde_json Display; `tools` prints it on stderr through `render::withheld_line` (`harness-cli/src/lib.rs:4030`). Pre-existing (identical at 95cb41f). Reached by `tools`/`run --substrate S --allow-program P` against a peer whose `exec.argv-only` or `workspace.guarded-io` fact is a string.
   - CHANGELOG claim. `CHANGELOG.md:17` says no daemon byte reaches the terminal raw any more; the leak above contradicts it. Introduced by aec6a5d.
   - Backslash ambiguity. Since aec6a5d leaves `\` raw, a raw ESC and the literal text `\u{1b}`, and a raw LF and a literal `\n`, display the same. No terminal effect. Introduced; 4850c06 kept them apart.
5. Could not break: Unreadable and Refused Display at every construction site; the CLI status-0 adopt arm and other adopt arms; `tools.rs:451`; Debug output; embedded.rs; nothing depended on the old Unreadable wording (CLI lib 244/244 green).

```findings
- file: crates/harness-substrate/src/withheld.rs
  line: 87
  category: boundary
  severity: warning
  verdict: NEEDS-CHANGE
  origin: pre-existing
  message: describe() quotes a daemon fact value with serde_json Display into the withheld reason, which tools/run print on stderr with DEL, C1 CSI and U+202E raw
- file: CHANGELOG.md
  line: 17
  category: contract-drift
  severity: warning
  verdict: NEEDS-CHANGE
  origin: introduced
  message: the Fixed line claims no daemon byte reaches the terminal raw any more, which the withheld-note path contradicts
- file: crates/harness-substrate/src/lib.rs
  line: 330
  category: property
  severity: note
  verdict: CONFIRMED
  origin: introduced
  message: leaving the backslash raw makes inert non-injective, so a raw ESC or LF and its spelled-out escape text display identically
```

Routed by the coordinator in 0c58221: the withheld leak fixed (inert in describe, the CLI case green), the CHANGELOG line corrected, the injectivity finding filed as `story:substrate-escape-is-injective` with its two cases ignored under that id. No third pass (two-pass budget).
