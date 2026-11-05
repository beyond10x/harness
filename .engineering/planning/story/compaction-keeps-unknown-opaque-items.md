---
format: aep.planning-md/3
id: story:compaction-keeps-unknown-opaque-items
kind: story
status: draft
title: Compaction drops only opaque items that are reasoning
tags:
- from-finding
relations:
- informed_by: review-result:adversary-1104-u3-pass-1
- serves: vision:b10x-owns-its-loop
revision: 1
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
