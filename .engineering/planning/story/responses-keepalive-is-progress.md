---
format: aep.planning-md/3
id: story:responses-keepalive-is-progress
kind: story
status: implemented
title: Responses keepalive is progress, not conversation
summary: A live keepalive marker advances no turn, emits no warning, and is never replayed as opaque provider input.
relations:
- derived_from: epic:pinned-interfaces-honest
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T08:08:11Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T08:08:11Z", actor: "human:timo", revision: 4, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T08:58:16Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

The Responses decoder treats a `keepalive` event as unknown. Invariant 7 correctly preserves genuinely unknown events, but a transport progress marker is modeled protocol traffic: preserving it turns it into an opaque conversation item that can be replayed on the next turn, while the operator sees a false `unknown-stream-event` warning.

## Acceptance

`keepalive` is in the Responses accepted event inventory, advances no turn state, emits no stream event or warning, and contributes no replay item. A new immutable provider-wire contract cut pins the event and both checker halves pass.

## Evidence to collect

A decoder regression case, the synthetic contract fixture and manifest, the provider contract checker, and the full repository gate.
