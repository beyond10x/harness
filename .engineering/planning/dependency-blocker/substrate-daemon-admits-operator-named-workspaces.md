---
format: aep.planning-md/3
id: dependency-blocker:substrate-daemon-admits-operator-named-workspaces
kind: dependency-blocker
status: open
title: substrate's daemon refuses an operator-named workspace id on exec.start
relations:
- blocks: story:daemon-exec-start-adoption
revision: 1
---
The fix lives in another repository. `validate_exec_input` in beyond10x/substrate
(`crates/substrate-daemon/src/app/operations.rs:241`, cited by the story) refuses a workspace id
that is not `ws_` plus an alphanumeric tail, so the socket path refuses `exec.start` for an
adopted directory such as `harness`. Nothing in this repository can change that predicate
(AGENTS.md invariant 2: substrate is a pinned foundation, never a sibling path).

The story also carries an open question for the operator: whether substrate's wire contract
(*Identity of resources*, `ws_…`) changes with the daemon. That is a contract change in substrate
with its own review.

Clears when substrate releases a daemon that admits operator-named workspace ids, with its
contract bundles still verifying, and this repository pins that release.

Filed by the harness session on 2026-10-21 while selecting wave `idle-1021`.
