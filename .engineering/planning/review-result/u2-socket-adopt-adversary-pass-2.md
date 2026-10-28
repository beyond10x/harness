---
format: aep.planning-md/3
id: review-result:u2-socket-adopt-adversary-pass-2
kind: review-result
status: active
title: Adversary pass 2 on socket adopt (U2)
summary: '2 new findings, 0 carried, 1 red case: a profile''s toolchain refusal names an untyped flag'
relations:
- reviews: story:daemon-exec-start-adoption
revision: 1
---
## Pass

Adversary pass 2 on unit U2 (`unit/socket-adopt` at `e57897b`, correction `ea12a46`), 2026-10-28,
`aep:adversary` (opus), the second and last pass of the budget. Cases executed 308 to 311, red 1.
Origin: introduced 1. New test file `crates/harness-cli/tests/adversary_u2_profile_keys_p2.rs`,
3 cases. Pass-1 findings carried: 0 (all four resolved; `adversary_u2_profile_write.rs` 3/3 green).

Red output, the file alone:

```
the refusal does not name the profile the toolchain came from: error: `--toolchain` was refused: a socket-backed substrate cannot receive this process's declared toolchain roots
test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

## Attacked and could not break

- Other ways a profile reaches the embedded driver: named `-p` profiles, `chat`, `tools`.
- Widening under a socket: a profile's default write scope is never wider than an untyped one;
  `allow-program` fills only when nothing was typed.
- The 409 hint test catches the substring mutant, the dropped guard and a non-JSON body.
- Live with the fixed binary: adopt passes the id, root-name and origin checks; a second adopt gets
  409 with the hint.

```findings
[
{"file":"crates/harness-cli/src/lib.rs","line":1436,"category":"usability","severity":"warning","verdict":"CONFIRMED","origin":"introduced","message":"under a typed --substrate a profile's toolchains now refuse the run as \"`--toolchain` was refused\", naming a flag that was never typed, and no command-line value opts out (test a_profiles_toolchains_under_a_typed_socket_are_refused_naming_where_they_came_from)"},
{"file":"crates/harness-cli/src/lib.rs","line":3497,"category":"usability","severity":"note","verdict":"INFEASIBLE","origin":"introduced","message":"an Unreadable from the new adopt checks, raised after the daemon has written the row, is worded as \"refused by the substrate daemon\" and prints no id; no daemon serving adopt is known to reach it (reported as suspected)"}
]
```
