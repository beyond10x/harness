---
format: aep.planning-md/3
id: story:vllm-reasoning-events-unpinned
kind: story
status: rejected
title: vLLM's reasoning stream events are outside the pinned subset
summary: '301 warnings in a two-turn run, and no ReasoningDelta ever fires: the pin knows OpenAI''s reasoning_summary_text names, vLLM sends reasoning_text.'
relations:
- informed_by: verification-report:openai-responses-on-vllm
- derived_from: epic:wire-pins-from-live-bytes
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: contracts/provider-wires/openai-responses
- confidence: inferred
  path: crates/harness-responses/src/lib.rs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-09-15T17:26:33Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "rejected", at: "2026-10-20T05:54:32Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"review_outcome":1}}}
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

A vLLM fixture/run containing the four observed events (response.reasoning_text.delta, response.reasoning_part.added, response.reasoning_text.done and response.reasoning_part.done) emits ReasoningDelta for the delta and no unknown-stream-event warning for those four admitted names, while an unrecognized response.reasoning_* event remains preserved and warned about; a new immutable contract version and the changelog carry that exact accepted subset.

## Rejected 2026-10-20

Surface gone and subject covered by llm. `crates/harness-responses` was deleted when harness
moved onto llm 0.8.0 (`story:harness-builds-on-llm-0-7`, merged as `8e5c17a9`). llm's Responses
decoder admits the four vLLM reasoning events this story named: `ACCEPTED_STREAM_EVENTS` has 23
names and adds `response.reasoning_part.*` and `response.reasoning_text.*` (parity row R2,
`llm-responses/src/stream.rs:25`, test `every_event_the_pin_declares_accepted_is_actually_interpreted`
at `llm-responses/tests/projection.rs:762`); an unknown `response.reasoning_*` event is preserved
and warned about through the bridge (invariant 7). A new provider-wire contract version is llm's
to cut, not this repository's. Decided by session `adr-changes` under the owner's exception 5:
"move each of the 4 stories to rejected only where llm covers its subject (cite the llm parity
row or test)".
