---
format: aep.planning-md/3
id: epic:harness-on-llm-and-tools
kind: epic
status: active
title: Harness is kept, builds on llm and gives its tools to tools (Atlas ADR 0097)
summary: ADR 0096 keeps harness as the in-house L1 harness; ADR 0097 moves its model calls onto llm and its tool layer into the tools repository.
relations:
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T01:57:56Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"review_outcome":1}}}
- {from: "proposed", to: "active", at: "2026-10-20T01:57:56Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"review_outcome":1}}}
---
## Context

Atlas ADR 0096 (2026-10-20, accepted) gives the agent stack one owner per layer and keeps this
repository as the in-house harness at L1, reached through metaharness like Claude Code and Codex.
ADR 0097 (2026-10-20, accepted) says what changes here, quoting the owner's decisions of 2026-10-20:
"i also think we ARE keeping harness and not deprecate it"; "harness should depend on llm"; "the
tools from harness must move into the tools repo".

Its decisions, as written (`atlas/architecture/adr/0097-harness-is-kept-builds-on-llm-and-gives-its-tools-to-tools.md`,
atlas main `5eb1abcd`):

1. harness stays the in-house harness (L1).
2. harness builds on llm: "The four wire and credential crates go; the bridge from the spike branch
   is rebased onto llm 0.7.0 (Rust 1.98, as llm requires). Compaction, mid-stream retry, the run
   ledger and approvals stay in harness."
3. harness's tools move into `tools`: "`harness-tools` and `harness-toolchain` become `tools` crates
   under the `tool-contract/1` profile (ADR 0095 phases T1 and T2), and harness consumes them."

Consequences the ADR states: "harness's six consumers keep their pins; a consumer moves to the new
tags in its own wave"; "harness's wave: rebase the spike, delete the wire crates, adopt `tools`
once T1 ships".

What already exists: branch `spike/builds-on-llm-0.1.7` (`8989d138`, one commit on top of
`origin/main` `3169042f`, not merged) deleted `harness-messages`, `harness-responses`,
`harness-http` and `harness-credential`, added `crates/harness-cli/src/llm_port.rs` (701 lines) and
passed 928 tests with 4 ignored (commit message of `8989d138`). The `tools` repository holds no
crate and no tag (`tools` main `dabd034`, `Cargo.toml` has no members; `git tag` empty).

## Outcome

This repository's model calls go through llm's client crates and its tool layer comes from the
`tools` repository, with the loop features ADR 0097 names (compaction, mid-stream retry, the run
ledger, approvals) still owned here.

## Scope

- `story:harness-builds-on-llm-0-7`: decision 2.
- `story:harness-consumes-tools-crates`: decision 3, the harness half of ADR 0095 phase T2. It
  waits for `tools` to release its first crates (`dependency-blocker:tools-has-no-release`).

## Out of scope

- A consumer's move to the new tags (ADR 0097 § Consequences): agentide, agent-platform,
  metaharness, uilab, atlas `integration/harness-plugins` and platform `runtime/inference` move in
  their own waves.
- metaharness's own tool surface following harness's tools into `tools` (ADR 0097 decision 3, second
  sentence): metaharness's plan.
- llm's parity record and status for the cutover: llm's `story:harness-cutover-on-llm-0-7`.

## Done when

Both stories are `implemented`. The release tag a consumer moves to (ADR 0097 § Consequences) is a
step outside this set: `AGENTS.md` § Releases, taken after each story's wave closes and asked for
separately, because a wave never cuts a tag (`aep:implementing`, wave mode). Neither story owns it.
