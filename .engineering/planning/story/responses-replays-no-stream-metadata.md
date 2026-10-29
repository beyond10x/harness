---
format: aep.planning-md/3
id: story:responses-replays-no-stream-metadata
kind: story
status: draft
title: A Codex run survives the stream's response.metadata event
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
revision: 1
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
