---
format: aep.planning-md/3
id: story:substrate-pin-0-14
kind: story
status: implemented
title: Harness embeds Substrate 0.14.0, the newest release
summary: substrate-host and substrate-wire move from 0.13.0 to 0.14.0
tags:
- d142
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: crates/harness-substrate/Cargo.toml
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-10-28T14:51:06Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-28T14:51:06Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-28T17:43:41Z", actor: "human:timo", revision: 7, decided_on: {"recorded":{"test_result":1}}}
---
## Context

The operator, 2026-10-05: "ALWAYS upgrade to most recent ESS", applied to Substrate by item D142
(2026-10-28): "Move harness's Substrate pin to the newest released Substrate if it is behind."
Harness pins `b10x-substrate-host` and `b10x-substrate-wire` `=0.13.0` at `c2b95f28`
(`crates/harness-substrate/Cargo.toml`); the newest Substrate release is `0.14.0`, tag commit
`0b9d50c6105cacc7f959eb41d5e65f252084a282` (`gh release list -R beyond10x/substrate`, 2026-10-28).
0.14.0 is additive for a struct literal that names every field: `ConfinementProfile` gains
`workspace_access` and `output_retention`, `RunOutput` gains `stdout_kept` and `stderr_kept`
(substrate `CHANGELOG.md` § `[0.14.0]`).

## Acceptance

1. Both substrate crates are pinned by the `0.14.0` tag commit with an exact `=0.14.0` version,
   and `Cargo.lock` carries one copy of each.
2. `cargo xtask gate` exits 0 on the integration branch, including
   `cargo test -p b10x-harness-substrate --locked --test conformance`.
3. `CHANGELOG.md` `[Unreleased]` names the move.

## Scope

`crates/harness-substrate/Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, and any harness source a
renamed or added substrate field forces.
