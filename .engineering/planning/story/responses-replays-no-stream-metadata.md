---
format: aep.planning-md/3
id: story:responses-replays-no-stream-metadata
kind: story
status: implemented
title: A Codex run survives the stream's response.metadata event
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T12:51:13Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-29T12:51:13Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-29T12:51:13Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":2}}}
---
## Evidence

- `measurements/runs/r1-live-codex-2026-10-29.jsonl` and its `.stderr`: `-p codex` on
  `gpt-5.6-sol`, `llm` 0.16.0. Turn 1 answered with one `unknown-stream-event` warning; turn 2 ended
  the run with `model wire refused: Refused: HTTP endpoint returned status 400`.
- A second run through a local recording proxy (2026-10-29, not committed): the turn-2 request's
  `input[6]` was `{"type":"response.metadata", ...}`, and the endpoint answered
  `invalid_request_error`, code `invalid_value`, param `input[6]`: "Invalid value:
  'response.metadata'".
- `llm` 0.16.0 `crates/llm-responses/src/stream.rs`: a stream event outside the accepted set is
  warned about, turned into an opaque item and pushed into the turn's output items; the loop replays
  opaque items verbatim (invariant 5).

## Context

`response.metadata` is a stream event, not an output item, and the endpoint accepts no such input
type. Replaying it breaks every tool-using Codex run on its second turn. Harness must not filter or
reinterpret an opaque item (invariant 5), so the fix belongs in `llm-responses`: an unmodelled
stream event is preserved and reported but does not become conversation state, or
`response.metadata` is modelled as a non-conversation event. Harness then re-pins `llm`.

## Acceptance

A tool-using `-p codex` run through the re-pinned `llm` completes past turn 2 on a live Codex login,
recorded as `vendor_live` under `measurements/runs/`, with the `response.metadata` event still
reported as a warning or as a modelled event and absent from the replayed input.

## Resolution

`llm` 0.17.0 lists `response.metadata` among the stream events `llm-responses` accepts and does not
act on (`crates/llm-responses/src/stream.rs`, `ACCEPTED_STREAM_EVENTS`), so the event no longer
becomes an opaque item and the loop has nothing of it to replay. It is a modelled event rather
than a warning: the live records carry no `unknown-stream-event` warning. Harness re-pins `llm`.

- Port test: `crates/harness-llm/tests/llm_0_17.rs`,
  `a_codex_metadata_event_does_not_become_an_item_the_next_turn_replays`, a loopback Responses
  stream with a `response.metadata` event: the answer is carried and no item carries the event.
- Live, 2026-10-29, Codex login, `gpt-5.6-sol`, `-p codex --surface flat`:
  `measurements/runs/r2-live-codex-{1,2}-llm017-2026-10-29.jsonl`, `vendor_live`. Both completed
  in 3 turns with the right answer (5 and 4 tool calls). Whether the endpoint sent the event on
  these runs is not observed; no recording proxy ran.
- Open in `llm`: a metadata item saved in a session under 0.16.0 is still sent on resume (draft
  `story:saved-codex-metadata-item-on-resume` there).
