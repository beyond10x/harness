---
format: aep.planning-md/3
id: story:compaction-shrinks-a-reasoning-heavy-conversation
kind: story
status: implemented
title: Compaction reaches its target when the weight is reasoning
summary: Freed 153 of 56,000 bytes when the weight was opaque reasoning items (measured on the scripted model)
relations:
- decomposes: epic:embedded-by-a-consumer
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/tests.rs
revision: 8
transitions:
- {from: "draft", to: "proposed", at: "2026-11-04T07:29:56Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-04T07:29:56Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-11-05T01:41:02Z", actor: "human:timo", revision: 8, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Context

Measured on 2026-10-26 with the scripted model (probe P2 of wave llm-0-16-final-line, deleted
after the run): three 20 kB reasoning items, a declared window of 40,000 tokens and a reported
input of 34,000. `compact_run` needed to free 56,000 bytes and freed 153 (60,660 to 60,507); a
summary turn was spent and folded 6 items, and 60,216 of the 60,507 bytes left were opaque items.
Elision never touches opaque items and the summary re-inserts all of them, so a conversation whose
weight is reasoning is not made smaller by compaction at all, and the next request meets the
provider's window.

A reasoning model on the Responses route carries one encrypted reasoning item per turn. Whether
that weight is what made the 2026-10-26 trial's post-compaction requests fail is **inferred**: the
three runs that ended after `conversation-compacted` left no transcript.

## Outcome

When compaction cannot reach its target because the weight is opaque, the oldest opaque items are
dropped together with the round trips they belong to, and the drop is announced like elision.

## Acceptance

1. The P2 fixture (three 20 kB reasoning items, window 40,000, reported 34,000) leaves the
   conversation at or under the target after one compaction.
2. A `conversation-compacted` warning names how many opaque items went.
3. No request after it carries an opaque item without the item it preceded.

## How the Outcome was met (wave 2026-11-04)

Recorded by the wave coordinator from `review-result:adversary-1104-u3-pass-1` (finding at
`crates/harness-loop/src/lib.rs:1452`). The Outcome above says opaque items are dropped "together
with the round trips they belong to". The implementation drops a reasoning item alone and keeps
the call and result it preceded: a provider requires a reasoning item to be followed by its item,
not the reverse, and dropping the call would hide a result the model just asked for. Acceptance 3
holds as written. Reasoning is dropped only where dropping it reaches the target (correction 1);
the count is in the `conversation-compacted` warning, and in the event only once
`story:compacted-event-counts-dropped-reasoning` lands.
