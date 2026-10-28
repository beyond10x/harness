---
format: aep.planning-md/3
id: dependency-blocker:substrate-daemon-admits-operator-named-workspaces
kind: dependency-blocker
status: cleared
title: substrate's daemon refuses an operator-named workspace id on exec.start
relations:
- blocks: story:daemon-exec-start-adoption
revision: 3
transitions:
- {from: "open", to: "cleared", at: "2026-10-28T14:50:07Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"artifact":1}}}
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

## Cleared, 2026-10-28

Delivered differently from how this blocker framed it. Substrate ADR 0035 (accepted 2026-10-17,
`adr/0035-an-operators-directory-is-served-after-an-explicit-adopt.md` in beyond10x/substrate)
keeps the wire `workspace-id` at `^ws_[A-Za-z0-9]+$` and adds an explicit `workspace.adopt`
(`POST /v1/workspaces/adopt`, input `root_name` and `labels`). The adopted directory gets a minted
`ws_…` id, and every route, `exec.start` included, addresses it by that id. That also answers the
story's open question: the contract's id vocabulary does not change.

- Shipped in substrate `0.7.17` (tag commit `00fa634c`): `CHANGELOG.md` § `[0.7.17]` lists
  `workspace.adopt`; `crates/substrate-daemon/src/app/routes.rs` routes `/v1/workspaces/adopt` at
  `0.7.17`, `0.13.0` and `0.14.0` (read with `git grep` at each tag, 2026-10-28).
- Harness already pins a release that carries it: `crates/harness-substrate/Cargo.toml` pins
  `0.13.0` at `c2b95f28` (`origin/main` `c715947`).

What remains is the harness side, which `story:daemon-exec-start-adoption` now asks for: call
adopt over the socket and run in the minted id. Cleared on the orchestrator's item D142
(`approval-record:wave-substrate-adopt`).
