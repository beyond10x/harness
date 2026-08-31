---
format: aep.planning-md/1
id: story:responses-keepalive-is-progress
kind: story
status: implemented
title: Responses keepalive is progress, not conversation
summary: A live keepalive marker advances no turn, emits no warning, and is never replayed as opaque provider input.
relations:
- derived_from: epic:pinned-interfaces-honest
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Defect

The Responses decoder treats a `keepalive` event as unknown. Invariant 7 correctly preserves genuinely unknown events, but a transport progress marker is modeled protocol traffic: preserving it turns it into an opaque conversation item that can be replayed on the next turn, while the operator sees a false `unknown-stream-event` warning.

## Acceptance

`keepalive` is in the Responses accepted event inventory, advances no turn state, emits no stream event or warning, and contributes no replay item. A new immutable provider-wire contract cut pins the event and both checker halves pass.

## Evidence to collect

A decoder regression case, the synthetic contract fixture and manifest, the provider contract checker, and the full repository gate.
