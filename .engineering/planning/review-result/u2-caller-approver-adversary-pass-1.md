---
format: aep.planning-md/3
id: review-result:u2-caller-approver-adversary-pass-1
kind: review-result
status: active
title: Adversary pass 1 on the caller approver (U2)
summary: '4 findings, 2 red cases: reused call id, uninterruptible wait; docs written at integration'
relations:
- reviews: story:approver-delegates-to-caller
revision: 1
---
## Pass

Adversary pass 1 on unit U2 (`unit/caller-approver`, uncommitted tree on `1751a95`), 2026-10-26,
`aep:adversary` (opus). Cases executed 1072 to 1083, red 2. Origin: introduced 4, pre-existing 0.
New test file `crates/harness-cli/tests/caller_adversary.rs`, 11 cases.

## Findings

| # | file:line | verdict | finding | route |
|---|---|---|---|---|
| 1 | `crates/harness-cli/src/caller.rs:211` | INFEASIBLE (no wire reuses ids) | the decision binds only `call_id`; a stale `allow` left in the pipe approves a later call reusing the id | fixed in correction 1: an id already decided is refused |
| 2 | `crates/harness-cli/src/caller.rs:274` | CONFIRMED | a run blocked on the caller's decision cannot be interrupted: SIGINT only sets the cancel token | fixed in correction 1: reader thread, `recv_timeout` against the token |
| 3 | `CHANGELOG.md:8` | NEEDS-CHANGE | no entry for `--approve caller` and contract `2026-10-26` | coordinator wrote it at integration |
| 4 | `AGENTS.md:166` | NEEDS-CHANGE | *Approvals* paragraph omits `caller` | coordinator wrote it at integration |

Not a finding but recorded: `caller.rs:173-176` doc comment overclaimed that no child can read
stdin; a substrate host process has no pid namespace. Reworded in correction 1.
