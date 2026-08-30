---
format: aep.planning-md/1
id: story:bridge-interrupt-race-pinned
kind: story
status: active
title: The bridge-mode interrupt test decides the same way under load
relations:
- derived_from: epic:gate-stays-trustworthy
- serves: vision:b10x-owns-its-loop
revision: 5
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

## Scope

Derived 2026-08-30 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/harness-cli` — test surface only, no `src/` — cited
- **Files:** `crates/harness-cli/tests/bridge_mode.rs:713-732` — cited, the named test
- **Files:** `crates/harness-cli/tests/bridge_mode.rs:684-711`
  `an_interrupt_queued_before_the_turn_starts_does_not_crash_the_server` — cited, a **sibling of the
  same race**, observed failing 2/2 under `bash scripts/gate.sh` at `c5bb2ed` on 2026-08-30 and
  passing 5/5 when the suite is run alone. Same `Endpoint::start("slow")`, same missing gate on a
  streamed event before the interrupt is written.
- **Files:** `crates/harness-responses/tests/fixtures/fake_responses.py:358-361` — cited, the `slow`
  scenario the story names as the other half of the race (`_send_sse(..., delay=0.5)`)
- **Symbols:** `an_unknown_request_mid_turn_is_refused_without_killing_the_server`,
  `an_interrupt_queued_before_the_turn_starts_does_not_crash_the_server` — cited
- **Symbols:** `Endpoint::start("slow")`, `Bridge::collect_until`, `Bridge::skip_to` — cited;
  `skip_to` is how the sibling interrupt tests already gate on `item/agentMessage/delta` before
  cancelling, and neither of these two does
- **Also likely:** `crates/harness-messages/tests/fixtures/fake_messages.py:394-397` — inferred, only
  if the fix adds a scenario rather than gating the test;
  `the_two_wires_serve_the_same_scenarios` (`crates/harness-messages/tests/provider_emulated.rs:160`)
  fails when one emulator declares a scenario the other does not
- **Also likely:** `crates/harness-app-server/src/transport.rs:71,176` — inferred, and least likely;
  production code is only touched if the pipelined frame's ordering cannot be pinned client-side
- **Documents:** none required — inferred; comparable test-only commits (`5493cea`, `65b59d3`) touch
  one file and no doc
- **Confidence:** high — the story cites the file, the test and the racing fixture by name, and the
  sibling failure was reproduced twice
- **Would collide with:** any unit touching `crates/harness-cli/tests/bridge_mode.rs`, and — if the
  fixture is changed — any unit reading the `slow` scenario of either emulator
  (`crates/harness-cli/tests/workflow.rs:1540`,
  `crates/harness-messages/tests/provider_emulated.rs:568`,
  `crates/harness-responses/tests/provider_emulated.rs:462`)

Open when scoped: which fix the acceptance intends — a client-side wait on `item/agentMessage/delta`
(one file) or a hold-until-signalled fixture scenario (four files, both emulators). The story says
"deterministic, not a wider margin" without naming the mechanism.
