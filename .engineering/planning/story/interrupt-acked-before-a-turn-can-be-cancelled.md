---
format: aep.planning-md/1
id: story:interrupt-acked-before-a-turn-can-be-cancelled
kind: story
status: draft
title: An interrupt acknowledged before the turn is cancellable stops nothing
summary: 'turn/started is notified before TurnControl is installed: an interrupt decoded in that window is acked, cancels nothing, and the client receives the answer it cancelled.'
relations:
- derived_from: epic:bridge-mode-proof
- informed_by: story:bridge-interrupt-race-pinned
revision: 2
---
## Evidence

`crates/harness-app-server/src/lib.rs:321-340`. `turn/start` responds with the turn id and notifies
`turn/started` inside a borrow of the writer, and **only then** builds the `TurnControl` and installs
it in `self.active`:

```rust
{
    let mut writer = self.wire.writer.borrow_mut();
    writer.respond(id, &json!({"turn": {"id": turn_id}}))?;
    writer.notify("turn/started", ...)?;
}
// ... TurnControl built here ...
if let Ok(mut active) = self.active.lock() {
    *active = Some(control.clone());
}
```

An interrupt decoded in that window finds `active == None`. `Wire::drain_control` acknowledges it,
and it cancels nothing: the turn then runs to completion and delivers the answer it was asked to
stop giving, while the client holds an acknowledgement saying it was stopped.

The comment immediately below the gap (`:332-334`) reasons about precisely this failure at the other
end of the turn — "an interrupt decoded just before the clear would be erased, and the turn it was
meant to stop would run to completion while the client held an acknowledgement" — and a fresh token
per turn was chosen to prevent it. The same hole is open at the start.

## How it was found, and how strong the evidence is

Measured, not inferred, while implementing `story:bridge-interrupt-race-pinned` on 2026-08-30.

- **Baseline**, as shipped, under oversubscription (`taskset -c 0-1`, `--test-threads=17`, 50 suite
  runs): **21 red of 50**, split 11/10 across two tests, always `left: "completed"` /
  `right: "interrupted"`, never both in one run — one shared cause.
- **Deterministic reproduction**: a temporary 200 ms sleep injected between `turn/started` and the
  point the turn becomes interruptible fails both tests 5 runs out of 5, unloaded.
- **Control**: with the same injection, the two interrupt tests that already gate on a streamed
  event survive a window a thousand times the natural one. That isolates the cause to the window,
  not to the fixture's pacing.

## Consequence

This is not a test defect. The tests were the only thing that noticed. **A real bridge client that
sends `turn/interrupt` promptly after `turn/start` returns can have its interrupt acknowledged and
ignored**, and will then receive the completed answer it cancelled. Promptly means within the time
it takes this process to build a `TurnControl` and take a mutex — small, but a client that pipelines
the two frames, as `an_interrupt_queued_before_the_turn_starts_does_not_crash_the_server` does, hits
it by construction rather than by luck.

`STATUS.md:17` claims an interrupt "is acted on when its frame is decoded, acknowledged between
streamed events, and distinguished from a client that merely vanished". The first clause is what
does not hold.

## Scope

`crates/harness-app-server/src/lib.rs` — moving `*active = Some(control)` above the `respond`/
`notify` block closes it. The `TurnControl` does not depend on anything the response computes, so the
move is mechanical; what needs argument is whether installing it before the client has been told the
turn exists creates a different window, and what happens to an interrupt for a turn id the client
cannot yet have seen.

Test-side, `crates/harness-cli/tests/bridge_mode.rs` already holds the two cases that catch it: with
this fixed they pass without their `skip_to` gate, which is the check that the fix is real rather
than the gate compensating for it.

## Acceptance

An interrupt decoded between `turn/start` being answered and the turn becoming interruptible cancels
that turn rather than being acknowledged and dropped — pinned by a case that injects the window and
fails without the fix — and `crates/harness-cli/tests/bridge_mode.rs`'s two interrupt tests pass
with their client-side gate removed.

## A test for it is already written

An adversarial pass against `wt/bridge-interrupt` wrote two cases before it was stopped, saved as
`.wave/records/adversary-bridge-cases.patch` (123 lines, against `crates/harness-cli/tests/bridge_mode.rs`):

- `an_acknowledged_interrupt_never_also_delivers_the_answer` — the assertion this story exists for.
  An interrupt that was acknowledged must not be followed by the completed answer.
- `a_request_arriving_mid_stream_is_refused_before_the_turn_ends`

They were never run, so their state is unknown; whoever takes this story applies the patch first and
finds out. A story that starts from an unrun test is still ahead of one that starts from a
description of one.
