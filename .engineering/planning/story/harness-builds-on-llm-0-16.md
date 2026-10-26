---
format: aep.planning-md/3
id: story:harness-builds-on-llm-0-16
kind: story
status: implemented
title: Harness builds on llm 0.16.0
summary: Seven llm crates move to 0.16.0; a Messages stream whose usage carries cost completes with a usage-extension warning
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: Cargo.toml
- confidence: cited
  path: README.md
- confidence: cited
  path: STATUS.md
- confidence: cited
  path: crates/harness-cli/tests/adversary_u1_llm_0_16_cli.rs
- confidence: cited
  path: crates/harness-llm/tests
- confidence: cited
  path: spec/COVERAGE.md
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-10-26T21:05:41Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-26T21:05:41Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-26T22:18:36Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":1,"verification":2}}}
---
## Context

llm released `0.16.0` on 2026-10-26 (tag on `33ae9001`,
https://github.com/beyond10x/llm/releases/tag/0.16.0). Its Messages client carries a streamed
`message_delta.usage.cost` as a `usage-extension` warning (`llm_messages::USAGE_EXTENSION`) instead
of refusing the stream. On 0.15.0 the same stream ended the run with `model wire refused:
Unsupported: Messages field is outside the declared subset: message_delta.usage.cost`. Harness pins
`0.15.0` (`Cargo.toml:74-80`).

Approved by session `b10x-chef-orchestrator`, 2026-10-26 (`approval-record:wave-llm-0-16-final-line`).

## Outcome

Harness builds on llm 0.16.0, and a streamed anthropic-messages turn whose usage carries `cost`
completes, with the extension visible in the run's record rather than dropped.

## Acceptance

1. `cargo xtask gate` exits 0 on a tree whose seven `llm-*` workspace dependencies pin
   `tag = "0.16.0"` and whose `Cargo.lock` holds no llm package at `0.15.0`.
2. A harness-level test drives the anthropic-messages client against a loopback stream whose
   `message_delta.usage` carries `cost`; the turn completes with its text and usage, and the sink
   receives one `warning` with code `usage-extension`.

## Decision: the warning is surfaced, not dropped

The harness passes llm's `usage-extension` warning through unchanged as a `warning` event (stderr
line, and one `{"kind":"warning",...}` line under `--json`), as it already does for every llm
stream warning (`crates/harness-llm/src/lib.rs`, `from_llm_event`). Invariant 7: an unmodelled
field is preserved and warned about, never dropped. The cost figure itself is not read as the
run's cost: the run's cost stays the one its rate card computes.

## Out of scope

- A new provider-wire contract version (invariant 14: wire bytes are llm's).
- Consumers' pins.
