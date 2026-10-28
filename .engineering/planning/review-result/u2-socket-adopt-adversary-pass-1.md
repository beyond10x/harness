---
format: aep.planning-md/3
id: review-result:u2-socket-adopt-adversary-pass-1
kind: review-result
status: active
title: Adversary pass 1 on socket adopt (U2)
summary: '4 findings, 2 red cases: a write profile silently replaces --substrate and --substrate-adopt with the embedded driver'
relations:
- reviews: story:daemon-exec-start-adoption
revision: 1
---
## Pass

Adversary pass 1 on unit U2 (`unit/socket-adopt` at `f7c02a0`), 2026-10-28, `aep:adversary`
(opus). Cases executed 302 to 305, red 2. Origin: introduced 1, pre-existing 0, undecided 1.
New test file `crates/harness-cli/tests/adversary_u2_profile_write.rs`, 3 cases (one green
control).

Red output, the file alone:

```
`[default] write = true` beside --substrate-adopt: a model turn started, so the run was not refused at the socket it was given:
test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.54s
```

## Attacked and could not break

- argv rules (`requires`, conflicts) on `run`, `chat` and `tools`; `--workspace-id`'s default does
  not trip the conflict.
- Live refusals against the 0.14.0 daemon come back by name with the daemon's body: 409 with the
  hint, 404 absent, 422 for a file.
- Client-side refusal (status 0, nothing sent) for `-x`, `a/..` and an empty name.
- The minted id is used for later routes; `--workspace-id <minted>` works afterwards.
- Client mutants (body shape, id pointer, root-name check) are caught by the unit's own tests.

```findings
[
{"file":"crates/harness-cli/src/lib.rs","line":1423,"category":"correctness","severity":"blocker","verdict":"CONFIRMED","origin":"introduced","message":"a profile with write = true sets substrate_embedded after clap's conflict check, so --substrate S --substrate-adopt X silently runs the embedded driver on --workspace, adopts nothing and starts a model turn, contradicting the flag's own contract (test a_write_profile_does_not_turn_a_typed_substrate_adopt_into_the_embedded_driver)"},
{"file":"crates/harness-cli/src/lib.rs","line":3344,"category":"correctness","severity":"warning","verdict":"NEEDS-CHANGE","origin":"undecided","message":"the same profile override makes a typed --substrate socket lose to the embedded branch of published(); the base source looks identical but was not run (test a_write_profile_does_not_turn_a_typed_substrate_socket_into_the_embedded_driver)"},
{"file":"crates/harness-cli/src/lib.rs","line":1132,"category":"usability","severity":"note","verdict":"CONFIRMED","origin":"introduced","message":"tools --substrate-adopt mints a workspace just to list tools, so a later run adopting the same name gets 409; the help text 'act in it' misleads for a listing"},
{"file":"crates/harness-cli/src/lib.rs","line":3470,"category":"usability","severity":"note","verdict":"NEEDS-CHANGE","origin":"introduced","message":"the 409 hint matches on status rather than on code workspace.already-exists, and blames an earlier adopting run that may not exist"}
]
```
