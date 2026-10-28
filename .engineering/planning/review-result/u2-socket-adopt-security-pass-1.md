---
format: aep.planning-md/3
id: review-result:u2-socket-adopt-security-pass-1
kind: review-result
status: active
title: Security review of socket adopt (U2)
summary: 'NEEDS-CHANGE: the minted workspace id is not checked against the published pattern'
relations:
- reviews: story:daemon-exec-start-adoption
revision: 1
---
## Pass

Security review of unit U2 (`unit/socket-adopt` at `f7c02a0`), 2026-10-28,
`aep:security-reviewer` (opus). Verdict NEEDS-CHANGE. Cases executed 302 to 304, red 2. Origin:
introduced 2. New test file `crates/harness-substrate/tests/security_u2_minted_id.rs`, 2 cases.

Red output, the file alone:

```
"ws_a/../../execs" was answered as the workspace to act in ("ws_a/../../execs"); it would reach `/v1/workspaces/ws_a/../../execs/files/...` and the stderr notice unchecked
a result naming root_name "other" origin "adopted" was answered as the adopt of `project` (ws_minted7)
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out
```

## Checked and found nothing wrong

- The confined exec request is unchanged apart from the workspace id.
- A refused adopt never falls back: live 409, 404, 422 and a client-refused `../escape` each end
  the run with exit 1.
- The root name is validated before anything is sent and travels only in the JSON body.
- No credential and no host path leaks on the stderr notice or in refusals.
- The live test `an_adopted_directory_is_reached_by_its_minted_id_and_runs_a_declared_program`
  passed against the real daemon.

```findings
[
{"file":"crates/harness-substrate/src/client.rs","line":538,"category":"integrity","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"workspace_adopt returns the daemon's /result/id unchecked against the published ^ws_[A-Za-z0-9]+$ pattern, so a non-conforming answer reaches the files route, the raw HTTP request line and the stderr notice verbatim (test a_minted_id_outside_the_published_workspace_id_pattern_is_not_answered_as_a_workspace)"},
{"file":"crates/harness-substrate/src/client.rs","line":539,"category":"integrity","severity":"note","verdict":"INFEASIBLE","origin":"introduced","message":"workspace_adopt does not compare the answered root_name and origin with the request, so a result for another directory is accepted as this adopt; no conforming daemon answers that way (test an_adopt_answered_for_another_root_name_or_origin_is_not_answered_as_this_directory)"}
]
```
