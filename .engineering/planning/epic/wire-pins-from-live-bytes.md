---
format: aep.planning-md/1
id: epic:wire-pins-from-live-bytes
kind: epic
status: draft
title: Both wire contracts are pinned from bytes a real provider sent
summary: Every provider-wire pin in contracts/ is emulator-derived; a live pin is a new dated version.
relations:
- decomposes: initiative:live-evidence
revision: 2
---
## Evidence

- `STATUS.md:16` — next evidence for the wire contracts: "re-pin the Responses wire from live bytes (see *Live provider*); re-pin `2026-08-29b`'s cache placement from a live Anthropic run rather than the emulator".
- `STATUS.md:21` — "pin a `2026-08-23` contract from live bytes rather than emulated ones; the current pin is still emulator-derived".
- `STATUS.md:19` — "Next evidence for this wire is the same as the contract's: **capture this route's bytes live**".
- `ROADMAP.md:122-128` — "Invariant 18 forbids promoting emulated evidence in place: a live pin is a **new dated version** cut from captured bytes, not an edit to this one."
- `AGENTS.md:81-86` — invariant 13: a contract version is immutable after release; a change cuts a new version directory.
- `AGENTS.md:98-104` — invariant 14: both halves hold — a Python checker against the fixtures, a Rust test against the code.
- `contracts/provider-wires/openai-responses/2026-08-22/` and `contracts/provider-wires/anthropic-messages/2026-08-29b/` — the two current pins, both derived from `scripts/`-driven emulators.

## Outcome

The bytes a consumer pins to are bytes a provider actually sent. Today they are bytes this
repository's own Python fixture server sent, checked against the code that talks to it — a closed
loop that agrees with itself.

## Scope

Two new dated contract versions, each cut from a captured live exchange, each keeping its emulated
predecessor in place as released (invariant 13). Capture tooling is in scope; editing an existing
version directory is not, at all.

## Out of Scope

The app-server profile pin: its counterpart is another program, not a provider, and its proof is
`epic:bridge-mode-proof`.

## Done When

`contracts/provider-wires/` holds one live-derived version per wire, each naming the run it was
captured from, with both halves of invariant 14 green against it.
