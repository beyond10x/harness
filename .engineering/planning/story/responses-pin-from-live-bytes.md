---
format: aep.planning-md/1
id: story:responses-pin-from-live-bytes
kind: story
status: draft
title: The Responses wire is pinned from bytes a real endpoint sent
relations:
- derived_from: epic:wire-pins-from-live-bytes
revision: 2
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

`contracts/provider-wires/openai-responses/<date>/` exists, its fixtures are bytes captured from an
authorized live endpoint rather than from `fake_responses.py`, the version says which run it came
from, and `python3 scripts/check-provider-wires.py` plus the crate's contract test pass against it.
