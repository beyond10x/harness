---
format: aep.planning-md/3
id: story:responses-pin-from-live-bytes
kind: story
status: rejected
title: The Responses wire is pinned from bytes a real endpoint sent
relations:
- derived_from: epic:wire-pins-from-live-bytes
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: contracts/provider-wires/openai-responses
- confidence: inferred
  path: crates/harness-responses/tests/contract.rs
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-09-15T17:26:32Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "rejected", at: "2026-10-29T11:11:28Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"review_outcome":1}}}
---
## Evidence

- `STATUS.md:21` — "**first live run: 2026-08-23**, against `https://chatgpt.com/backend-api/codex` … Two turns, two tool round trips, usage reported, `finished{completed}`" and, as the next evidence, "pin a `2026-08-23` contract from live bytes rather than emulated ones; the current pin is still emulator-derived".
- `STATUS.md:11` — Responses wire next evidence: "characterize one authorized live endpoint and retain the evidence".
- `STATUS.md:16` — "re-pin the Responses wire from live bytes (see *Live provider*)".
- `contracts/provider-wires/openai-responses/2026-08-22/README.md:39` — the pin's *Fixtures* section: the bytes it holds come from `crates/harness-responses/tests/fixtures/fake_responses.py`.
- `AGENTS.md:116-117` — invariant 18: emulated evidence is never promoted, and no prose may imply a real provider was contacted.
- `ROADMAP.md:122-128` — a live pin is a new dated version cut from captured bytes, never an edit to an emulated one.

## Context

The 2026-08-23 run happened and found a defect the emulator could not — the whole workspace toolset
was named illegally for that wire, on turn one (`STATUS.md:21`). Its bytes were not kept, so the
document a consumer pins to still describes what this repository's own Python fixture server sends.
The gap between the two is exactly the class of defect that run found.

What is owed is not a new capability: it is a capture path (record the request and the stream of one
authorized run), a new dated version directory built from it, and the two halves of invariant 14
green against that directory.

## Acceptance

A new dated contracts/provider-wires/openai-responses/<date>/ contains sanitized fixtures captured from an authorized live endpoint, names the source run and its provenance, and passes both cargo xtask provider-contracts and cargo test -p b10x-harness-responses --test contract --locked. Leave released contract directories unchanged; historical emulator fixtures are not promoted to live evidence.

## Resolution

Rejected on 2026-10-29 under `approval-record:wave-live-pins-d200`. A new
`contracts/provider-wires/` version is no longer cut in this repository: invariant 14 in
`AGENTS.md` moved what is sent and accepted to `llm` with the wire crates (atlas ADR 0097). `llm`
covers the subject in `story:live-provider-captures` (implemented 2026-10-21, in `llm` 0.16.0, the
tag this workspace pins): `crates/llm-responses/tests/live_capture.rs` replays a live Codex text
turn. The harness side was observed live on 2026-10-29 and found a defect:
`measurements/runs/r1-live-codex-2026-10-29.jsonl` answered turn 1 and was refused on turn 2,
because the replayed input carried the stream's `response.metadata` event
(`story:responses-replays-no-stream-metadata`).
