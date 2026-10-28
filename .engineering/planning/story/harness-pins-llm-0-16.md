---
format: aep.planning-md/3
id: story:harness-pins-llm-0-16
kind: story
status: implemented
title: Harness pins llm 0.16.0, the newest release
summary: The seven llm crates move from 0.15.0 to 0.16.0, which carries a streamed Messages usage cost as a warning
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
  path: Cargo.toml
- confidence: cited
  path: STATUS.md
- confidence: cited
  path: crates/harness-llm/tests/messages_usage_cost.rs
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-28T15:49:46Z", actor: "human:timo", revision: 7}
- {from: "proposed", to: "active", at: "2026-10-28T15:49:46Z", actor: "human:timo", revision: 8}
- {from: "active", to: "implemented", at: "2026-10-28T17:43:41Z", actor: "human:timo", revision: 9, decided_on: {"recorded":{"test_result":1,"verification":1}}}
---
## Context

Added to wave substrate-adopt by `b10x-chef-orchestrator` on 2026-10-28 under the grant
(dependency upgrade): "pin llm 0.16.0 in harness. It replaces 0.15.0; 0.16.0 carries the
usage.cost fix 39346593, which the takt 0.17.0 trial needs. In that trial the anthropic arm got
0/2, exit 5: \"llm 0.15.0 still refuses message_delta.usage.cost\"."

Verified: llm release `0.16.0` (tag commit `33ae9001`, published 2026-10-26T20:43:32Z) is the
newest; `39346593` ("carry a streamed Messages usage cost as a usage-extension warning") is an
ancestor of the tag (compare: ahead 6, behind 0). Harness pins the seven `llm-*` crates at tag
`0.15.0` (`Cargo.toml:74-80`). Harness forwards `llm_core::StreamEvent::Warning` unchanged
(`crates/harness-llm/src/lib.rs:703`), so no source change is needed.

Open pull request beyond10x/harness#53 (2026-10-26, conflicting with `main`) carries its own
unmerged `story:harness-builds-on-llm-0-16`; this story lands the pin through the current wave,
and the test file `crates/harness-llm/tests/messages_usage_cost.rs` is taken from that branch
(`e9ed0623`) with only its story id changed.

## Acceptance

- `Cargo.toml` pins every `llm-*` crate at tag `0.16.0`; `Cargo.lock` changes only those seven
  packages.
- `cargo test -p b10x-harness-llm --test messages_usage_cost` passes: a numeric and an object
  `message_delta.usage.cost` each complete the turn with one `usage-extension` warning.
- The full gate is green on the integration branch.

## Out of scope

A harness release is cut by its own process after the wave merges; metaharness and takt follow
that tag.
