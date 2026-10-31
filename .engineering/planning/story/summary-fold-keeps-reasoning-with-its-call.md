---
format: aep.planning-md/3
id: story:summary-fold-keeps-reasoning-with-its-call
kind: story
status: draft
title: A summary fold keeps no reasoning item apart from the item it preceded
summary: summarise re-inserts folded reasoning items after the summary, detached from their folded calls (measured)
relations:
- decomposes: epic:embedded-by-a-consumer
revision: 1
---
## Context

Measured on 2026-10-26 with the scripted model (probe P1 of wave llm-0-16-final-line, deleted
after the run): after a summary fold the next request's items were
`[user, summary, opaque:r1, opaque:r2, call:c2, result:c2]`. `summarise`
(`crates/harness-loop/src/lib.rs`) re-inserts every opaque item of the folded range after the
summary, but the calls those reasoning items preceded were folded into the summary, so `r1` is now
followed by `r2` instead of its call. `fold_end` keeps a fold boundary off a reasoning item whose
call is in the tail; nothing checks the reasoning items inside the fold.

On the Responses route a reasoning item must be followed by the item it precedes
(`assert_replayable` in `crates/harness-loop/src/tests.rs` states the same rule, and checks only
that the record does not end on one). Whether the provider of the 2026-10-26 trial refused such a
request is **inferred, not observed**: three trial runs ended right after a
`conversation-compacted` warning with no terminal record, and their transcripts were not kept.

## Outcome

A summary fold leaves no reasoning item separated from the item it preceded: the folded range's
reasoning items are either dropped with their calls (said out loud, as elision is) or kept with
them.

## Acceptance

1. A loop test folds a range holding `reasoning, call, result` pairs; the next request holds no
   opaque item whose next item is another opaque item or a user item.
2. `assert_replayable` checks that every opaque item is followed by a call or assistant text.
