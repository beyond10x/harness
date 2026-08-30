---
format: aep.planning-md/1
id: story:vllm-reasoning-events-unpinned
kind: story
status: draft
title: vLLM's reasoning stream events are outside the pinned subset
summary: '301 warnings in a two-turn run, and no ReasoningDelta ever fires: the pin knows OpenAI''s reasoning_summary_text names, vLLM sends reasoning_text.'
relations:
- informed_by: verification-report:openai-responses-on-vllm
revision: 1
---
## Evidence

- One two-turn run against vLLM v0.27.1 emitted **307 warning lines** and roughly 30 real events:

  | count | event |
  |---|---|
  | 301 | `response.reasoning_text.delta` |
  | 2 | `response.reasoning_part.added` |
  | 2 | `response.reasoning_text.done` |
  | 2 | `response.reasoning_part.done` |

  each as `{"kind":"warning","code":"unknown-stream-event","message":"stream event `…` is outside
  the pinned subset and was skipped"}`.

- `crates/harness-responses/src/lib.rs:338` matches `response.reasoning_summary_text.delta` — the
  name **OpenAI** streams. vLLM streams `response.reasoning_text.delta`. Neither is wrong; the pin
  only knows one of them.
- `verification-report:openai-responses-on-vllm` — the run these counts come from.

## Context

The behaviour is correct under invariant 7: an unmodelled event is preserved and warned about rather
than dropped, and the run completed. Two things follow from it anyway.

**A person watching a long think sees nothing.** `StreamEvent::ReasoningDelta` exists so that
"a person watching a long think sees something happening"
(`crates/harness-loop/src/event.rs:217-223`). On this route it never fires, because the only event
that produces it is the one vLLM does not send. On a 27B model at 32k context that is the difference
between a visibly working run and a silent one.

**The `--json` record is unreadable.** 301 warning lines to 30 events is a ten-to-one noise ratio in
the exact artefact a caller is meant to parse, and it scales with reasoning length, so a real run on
the 27B model is worse than this one.

## Scope

`crates/harness-responses/src/lib.rs` — the stream-event match. A new pinned contract version under
`contracts/provider-wires/openai-responses/<date>/` if the accepted event set changes (invariant 13:
never an edit to a released directory). `CHANGELOG.md` in the same change (invariant 14).

Adding `response.reasoning_text.*` beside the `reasoning_summary_text.*` names is the smaller of two
options. The other — a general rule for `response.reasoning_*` — would be pinning a pattern rather
than a subset, which is a different claim about what this wire accepts and needs its own argument.

## Acceptance

A run against vLLM emits `ReasoningDelta` events and no `unknown-stream-event` warning for any
`response.reasoning_*` event, with the pin and the changelog carrying the change.
