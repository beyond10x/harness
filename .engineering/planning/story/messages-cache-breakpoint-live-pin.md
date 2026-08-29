---
format: aep.planning-md/1
id: story:messages-cache-breakpoint-live-pin
kind: story
status: draft
title: The rolling cache breakpoint is pinned from a live Anthropic run
relations:
- derived_from: epic:wire-pins-from-live-bytes
revision: 2
---
## Evidence

- `STATUS.md:19` — Messages wire: "Next evidence for this wire is the same as the contract's: **capture this route's bytes live** — `2026-08-29b` is still emulator-derived, and its rolling cache breakpoint is the part most worth pinning from a real run".
- `STATUS.md:16` — "re-pin `2026-08-29b`'s cache placement from a live Anthropic run rather than the emulator".
- `ROADMAP.md:122-128` — "**a live contract version for the Anthropic route.** The run below happened; its *bytes* were not captured … The cache-breakpoint placement `2026-08-29b` introduces is the part most worth capturing live: the measurement that argued for it is a hit-rate series, and the pin itself is emulated."
- `contracts/provider-wires/anthropic-messages/2026-08-29b/README.md:10-41` — what changed from `2026-08-29`: the rolling `cache_control` breakpoint on the last block of the last message.
- `STATUS.md:20` — the authorized run that did happen: 2026-08-29, `claude-haiku-4-5-20251001`, three turns, two tool calls, with a deliberately invalid token to the same endpoint answering `401 authentication_error` as the control.

## Context

The Anthropic route is the one route with an authorized run behind it, and the pin in force for it is
still emulator-derived. The specific thing worth capturing is the cache breakpoint: it is the one
part of the request whose *placement* the provider reacts to, the argument for it was a hit-rate
series, and an emulator agrees with whatever placement the code chose.

Same shape as the Responses story, one route along: capture, cut a new dated directory, leave
`2026-08-29b` standing as released (invariant 13, `AGENTS.md:81-86`).

## Acceptance

A new dated `contracts/provider-wires/anthropic-messages/<date>/` holds a request captured from a
live run, showing the breakpoint where this harness places it and the provider's own cache figures
in the response, with both halves of the contract check green against it.
