---
format: aep.planning-md/1
id: story:bridge-interrupt-race-pinned
kind: story
status: draft
title: The bridge-mode interrupt test decides the same way under load
relations:
- derived_from: epic:gate-stays-trustworthy
revision: 2
---
## Evidence

- `docs/reviews/2026-08-29-code-review-2.md:38-41` — "Still open: … `bridge_mode::an_unknown_request_mid_turn_is_refused_without_killing_the_server` failed once under a loaded gate (`completed` where `interrupted` was expected) and passed 5/5 alone — a race between the `slow` fixture and the interrupt, **not yet pinned**."
- `crates/harness-cli/tests/bridge_mode.rs` — the suite it is in: 17 tests driving the shipped binary over pipes (`STATUS.md:61`).
- `STATUS.md:17` — what the test guards: "An interrupt is acted on when its frame is decoded, acknowledged between streamed events, and distinguished from a client that merely vanished".
- `docs/reviews/2026-08-29-code-review-2.md:19` — the precedent for the fix: two deadline tests with a 40 ms budget against 60 ms calls were re-based to 200 ms / 300 ms for the same reason, one scheduling stall on a shared runner.
- `AGENTS.md:203-212` — the gate is run before every commit, so an intermittently red test is paid for on every commit.

## Context

One recorded intermittent failure, in the one area with no external proof at all. The observed
symptom is the interesting one: the turn reported `completed` where `interrupted` was expected,
which under load is indistinguishable from an interrupt that genuinely arrived too late — so the
test cannot currently tell a real defect from a slow machine.

The review that found it also fixed two tests of exactly this shape by widening their timing margins;
this one was left because the race is between a fixture's pacing and the interrupt, not a plain
timeout.

## Acceptance

The test decides the same way under a loaded gate — pinned by making the interrupt's arrival
deterministic rather than by widening a margin — and 50 consecutive runs under oversubscription give
one answer.
