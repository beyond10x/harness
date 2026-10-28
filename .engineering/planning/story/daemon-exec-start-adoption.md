---
format: aep.planning-md/3
id: story:daemon-exec-start-adoption
kind: story
status: active
title: An adopted workspace can run an exec over the daemon path
relations:
- derived_from: epic:adoption-follow-ups
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-substrate/src/client.rs
- confidence: inferred
  path: crates/harness-substrate/tests/conformance.rs
- confidence: cited
  path: crates/harness-substrate/tests/live.rs
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-09-15T17:26:30Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-10-28T14:50:44Z", actor: "human:timo", revision: 9}
---
## What is missing

`--substrate-embedded` can now serve a directory the operator already owns (harness `0c31438`,
substrate `0.2.2`). Over the **socket** path it cannot: `substrate-daemon`'s `validate_exec_input`
still requires the workspace id to begin `ws_` with an alphanumeric-only tail, so an adopted
directory named `harness` or `engineering-protocols` can be read and written and then refused an
`exec.start`.

- `crates/substrate-daemon/src/app/operations.rs:241` in `beyond10x/substrate` — the predicate.
- `crates/harness-substrate/src/embedded.rs:17` — the embedded driver does not import that crate,
  which is why the embedded path is whole and this one is not.

## Why it was left

Found while widening the host-side check, by a sub-agent reading further than the change asked for.
Deliberately not widened in the same wave: it is wire-contract-adjacent, and substrate's gate
verifies four pinned contract bundles (`0.1.0`–`0.4.0`). Changing a validation the wire contract
describes is its own change with its own review, not a rider on a host-side one.

## Acceptance

Re-scoped 2026-10-28 (wave substrate-adopt) to what substrate delivered. Substrate ADR 0035
keeps the wire `workspace-id` at `ws_…` and adds `workspace.adopt` (`POST /v1/workspaces/adopt`,
shipped in substrate `0.7.17`); an adopted directory gets a minted `ws_…` id and every route,
`exec.start` included, addresses it by that id. The open question below is answered by that ADR:
the id vocabulary does not change. The harness side is to call adopt.

1. `Client::workspace_adopt(root_name)` (`crates/harness-substrate/src/client.rs`) posts
   `POST /v1/workspaces/adopt` with a fresh operation id and `{"root_name", "labels": {}}` and
   answers the minted id. A name outside substrate's root-name set is refused before any request;
   the daemon's refusals come back unchanged as `SubstrateError::Refused`.
2. `--substrate-adopt <ROOT_NAME>` beside `--substrate <socket>` adopts that directory and runs the
   write and execute tools in the minted id; it refuses the run by name when adopt is refused, and
   conflicts with `--substrate-embedded` and an explicit `--workspace-id`. The argv change is cut
   as a new `contracts/cli/b10x-harness/` version.
3. Against a real substrate daemon (the `0.14.0` release binary), an adopt of a directory named
   without `ws_` followed by an exec over the socket returns the program's output
   (`crates/harness-substrate/tests/live.rs`, run with `B10X_SUBSTRATE_SOCKET`).

## Open question for the operator

Whether the wire's own id vocabulary should change with it. Today a workspace id is `ws_…` by the
contract's *Identity of resources*; the host now accepts an operator-named root, so the contract and
the daemon disagree about what an id may look like. Widening the daemon without saying so in the
contract would leave that disagreement written down nowhere.
