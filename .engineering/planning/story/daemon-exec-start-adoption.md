---
format: aep.planning-md/1
id: story:daemon-exec-start-adoption
kind: story
status: draft
title: An adopted workspace can run an exec over the daemon path
relations:
- derived_from: epic:adoption-follow-ups
revision: 2
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

A run over `--substrate <socket>` against a workspace named `harness` starts an exec and returns
its output, and the four contract bundles still verify.

## Open question for the operator

Whether the wire's own id vocabulary should change with it. Today a workspace id is `ws_…` by the
contract's *Identity of resources*; the host now accepts an operator-named root, so the contract and
the daemon disagree about what an id may look like. Widening the daemon without saying so in the
contract would leave that disagreement written down nowhere.
