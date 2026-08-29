---
format: aep.planning-md/1
id: epic:bridge-mode-proof
kind: epic
status: draft
title: A real bridge client drives this binary
summary: 'Phase 2''s exit: the two processes have never spoken and no gate compares their inventories.'
relations:
- decomposes: initiative:driven-from-outside
revision: 2
---
## Evidence

- `STATUS.md:17` — bridge mode: the pinned subset is served, and the next evidence is "**run `runtime/agent`'s real bridge against this binary.** Everything so far is this component's own client, written from the bridge's source; the two processes have never spoken, and no gate compares the two inventories".
- `ROADMAP.md:23-28` — Phase 2: "**Status: implemented; the cross-component proof is open.**"
- `ROADMAP.md:42-44` — Phase 2 exit: "the existing bridge, pointed at this binary instead of `codex`, drives a turn".
- `AGENTS.md:105-109` — invariant 15: the method inventory here is a copy, "**Nothing in this repository checks the copy against the original**", re-reading it is a review obligation, "and the only thing that catches a mismatch is running the real bridge".
- `AGENTS.md:110-113` — invariant 16: the declared profile must be one the client actually offers; declaring the stable profile while emitting `item/tool/call` "yields a server that looks compatible and fails at the first tool call".
- `README.md:41` — "implemented; **no real external bridge has ever driven it**, and no gate compares the two method inventories".
- `contracts/app-server-profile/codex-app-server-stdio-v2-dynamic-operation-tools-experimental/2026-08-21/README.md:83` — the pin's own section "What these checks do **not** catch".
- `docs/reviews/2026-08-29-sota-comparison.md:93` — "bridge mode | never driven by a real client".

## Outcome

The bridge-mode server is known to work against the client it was written for, rather than against
this repository's reading of that client's source.

## Scope

One run of `runtime/agent`'s existing bridge with its command pointed at `b10x-harness app-server`,
its evidence retained, and whatever the run finds fixed. Invariant 2 forbids importing the client,
so the proof is a run and cannot be a test that reads the other side.

## Risks

Invariant 15 says a mismatch is invisible here by construction. The likeliest failure is not a
crash but a server that looks compatible until the first tool call (invariant 16) — so the run has
to include a tool call, not just a text turn.

## Done When

A turn driven by the real bridge completes against this binary, with at least one tool call, and
`STATUS.md`'s bridge row states the date it happened instead of stating that it has not.
