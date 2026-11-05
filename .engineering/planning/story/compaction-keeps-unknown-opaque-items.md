---
format: aep.planning-md/3
id: story:compaction-keeps-unknown-opaque-items
kind: story
status: implemented
title: Compaction drops only opaque items that are reasoning
tags:
- from-finding
relations:
- informed_by: review-result:adversary-1104-u3-pass-1
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: crates/harness-cli/tests/reasoning_wires.rs
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/tests.rs
- confidence: cited
  path: crates/harness-wire/src/item.rs
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T02:19:08Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-05T02:19:08Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-11-05T04:11:59Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

Found by `review-result:adversary-1104-u3-pass-1` (finding at `crates/harness-loop/src/lib.rs:1458`,
INFEASIBLE, nothing found that reaches it). The summary fold and `drop_reasoning` drop every
`Item::Opaque` as provider reasoning. The pinned llm also preserves unknown Responses output items
as `Opaque` (`llm-responses/src/stream.rs`, `output_item`). AGENTS.md invariant 7 says an
unmodelled output item is preserved and warned about, never dropped. Nothing harness publishes
today produces such an item (only function tools are published), so the case is unreached.

## Outcome

Compaction drops only opaque items it can tell are reasoning, and keeps any other opaque item, or
the record shows that no other kind of opaque item can reach the loop.

## Acceptance

- A loop test with a non-reasoning opaque item in the folded range and over target shows it kept
  after compaction, or a test pins that the wire never yields one.
