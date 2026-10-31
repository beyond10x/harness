---
format: aep.planning-md/3
id: story:bridge-turn-start-after-interrupt-on-every-path
kind: story
status: draft
title: Bridge mode serves a turn/start sent after an interrupt on every path
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: crates/harness-app-server/src/lib.rs
- confidence: inferred
  path: crates/harness-app-server/src/session.rs
revision: 2
---
## Context

Filed by the wave d288 coordinator from U2 correction 1 (commit `158863f`). That correction stopped
the control drain once a turn is asked to stop, so a `turn/start` sent right behind an interrupt
runs as the next turn. Two other paths still refuse such a request, and both predate the wave:

- `Wire::settle_interrupts` (`crates/harness-app-server/src/session.rs`) refuses a `turn/start`
  queued between two interrupts owed to the same turn (interrupt, turn/start, interrupt, all sent
  while one turn streams).
- `BridgeTools::await_response` refuses a `turn/start` sent after an interrupt while a tool call is
  outstanding ("cannot be served while a tool call is outstanding").

Fixing either needs the server to hold a deferred request for the main loop instead of answering it
on the spot.

## Outcome

In bridge mode, a `turn/start` sent after an interrupt is served as the next turn on every path:
between two interrupts owed to one turn, and while a tool call of the interrupted turn is
outstanding.

## Acceptance

- An in-process case sends interrupt, `turn/start`, interrupt during one streaming turn: both
  interrupts are answered before that turn's `turn/completed`, and the `turn/start` starts a second
  turn rather than receiving -32601.
- An in-process case sends interrupt then `turn/start` while a tool call is outstanding: the
  interrupted turn ends and the second turn starts, with no "tool call is outstanding" refusal.
