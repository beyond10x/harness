# `openai-responses` wire, version 2026-08-22

The exact subset of the OpenAI Responses API this harness speaks. Immutable once released: a change
to what is sent or accepted opens a new dated version rather than editing this one.

## What changed from 2026-08-21

Three request fields, all optional: `temperature`, `top_p`, and `reasoning` carrying an `effort`.
Nothing on the response side moved, so `turn-stream.sse` is byte-identical to the previous
version's.

A field nobody set is **absent**, not defaulted. Writing a provider's own default into the request
would take a decision that provider is entitled to make and change, make it ours, and make it
invisible — a request carrying `temperature: 1.0` looks exactly like one somebody chose. The
previous version pinned that these fields can be left out; this one pins what they look like when
they are set, which is why the fixture carries values rather than absences.

`effort` is **nested** under `reasoning`. A flat `reasoning_effort` is accepted by the transport and
ignored by the provider, which is the failure mode `each_sampling_field_travels_under_its_own_wire_name`
exists to prevent.

## Shape

| | |
| --- | --- |
| Endpoint | `POST {base_url}/responses` |
| Transport | HTTPS with a server-sent-event response body |
| Credential | `Authorization: Bearer …`, read at call time from an injected source |
| Stateful | No. `store: false`, and the whole conversation is replayed each turn |

`include: ["reasoning.encrypted_content"]` is always requested. Under `store: false` the provider
retains nothing, so without it the model loses its own reasoning across every tool round trip and
re-derives its plan on each call. Those items come back as opaque values tagged `openai-responses`,
are replayed byte for byte, and are never handed to another wire.

Sampling is sent on **every** turn for the same reason: the loop is stateless, so a value carried
only on the first request would apply only to the first request.

## Fixtures

Both are load-bearing, and each is checked from a different direction. Together they mean the
contract cannot drift from the code without something failing.

| File | Pins | Checked by |
| --- | --- | --- |
| `turn-request.json` | every field the harness sends, on a turn carrying an instruction, a person's input, a replayed reasoning item, one call, its result, and all three sampling fields | `crates/harness-responses/tests/contract.rs` builds it from the real projection and compares |
| `turn-stream.sse` | every event the harness interprets, framed as it arrives on the wire | the same test replays it through `decode_stream`, the exact code a live turn uses |
| `manifest.json` | the event list and each fixture's digest | `scripts/check-provider-wires.py` |
| `manifest.json` `request_fields` | the exact field set the harness sends | the same Rust test, not the Python checker |

The stream fixture must decode with **no** warnings. A warning there means an event in the pinned
set is no longer recognized, which is drift the digest check alone would not catch.

`output_items`, `usage_fields`, `endpoint`, `transport`, `streaming` and `stateful` are documented
here and in the manifest but **not** mechanically checked. Treat them as prose.

## Handling rules

- An output item outside `message`, `function_call` and `reasoning` is preserved as an opaque value
  and reported as a warning. Dropping it would leave a hole the next turn cannot see.
- A stream event outside the pinned set is reported as a warning and skipped.
- Arguments that are not valid JSON are a typed refusal. A half-parsed argument blob must never
  reach a tool, because the tool would act on a value the model did not send.
- A stream that ends before a terminal response is a truncation, never a completion.
- Absent usage stays absent. A zero would be a claim that no tokens were spent.
- A sampling value outside its range is refused here, before the request is sent. The round trip
  would otherwise cost a turn and come back as a vendor error string nobody can act on.

## What this contract does not say

That any endpoint **acts** on these three fields. It says they are sent, in these positions. An
endpoint may honour them, ignore them, or have fixed them elsewhere entirely — the self-hosted
gateway fixes thinking and effort when it launches a pod, so a per-request effort reaches it and
changes nothing. Which endpoint honours what is the route registry's question, not this one's.

## Conformance

`provider_emulated`. Every case runs against a deterministic local endpoint over a real socket. No
run against a real provider has happened, and nothing here may be read as evidence that one behaves
this way.

## Edited after release — 2026-08-24

This version's fixtures and manifest were rewritten once after `0.1.0` was tagged: the org-wide
brand sweep (`a54ec76`, `6aded80`) renamed the former-brand identifiers the fixtures carried and
re-pinned the digests to the new bytes (atlas ADR 0001, *Wire-visible identifiers*). That is the
one exception to invariant 13 — a released version is immutable — and it is recorded here so a
reader comparing this directory against the tag is not left guessing. Any further change to what
is sent or accepted cuts a new dated version; this directory is not edited again.
