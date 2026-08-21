# `openai-responses` wire, version 2026-08-21

The exact subset of the OpenAI Responses API this harness speaks. Immutable once released: a change
to what is sent or accepted opens a new dated version rather than editing this one.

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

## Fixtures

Both are load-bearing, and each is checked from a different direction. Together they mean the
contract cannot drift from the code without something failing.

| File | Pins | Checked by |
| --- | --- | --- |
| `turn-request.json` | every field the harness sends, on a turn carrying an instruction, a person's input, a replayed reasoning item, one call and its result | `crates/harness-responses/tests/contract.rs` builds it from the real projection and compares |
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

## Conformance

`provider_emulated`. Every case runs against a deterministic local endpoint over a real socket. No
run against a real provider has happened, and nothing here may be read as evidence that one behaves
this way.
