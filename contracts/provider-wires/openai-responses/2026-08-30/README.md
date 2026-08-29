# `openai-responses` wire, version 2026-08-30

The exact subset of the OpenAI Responses API this harness speaks. Immutable once released: a change
to what is sent or accepted opens a new dated version rather than editing this one.

## What changed from `2026-08-22`, and why

**One new top-level request field: `tool_choice`.**

The reason is the same on both wires and is stated once, in the sibling contract
(`contracts/provider-wires/anthropic-messages/2026-08-30/README.md`): a run under an output schema
that ends in prose reports nothing, and the seventh paid native walk (2026-08-30, Haiku 4.5) did
that on three of four attempts at one section under the words alone. The field is sent on at most
one turn per run — the turn a nudge opens.

| neutral value | this route sends |
| --- | --- |
| `auto` | *nothing.* The model choosing is this route's default |
| `required` | `"required"` — a bare string |
| named | `{"type": "function", "name": "<the tool>"}` |

**Two wires, two spellings, neither guessable from the other**: this route spells two of the three
as bare strings where the sibling spells all three as objects. That is the reason the neutral value
exists rather than the vendor's own shape being carried around.

A turn held to a tool it does not publish is refused before it is sent, by `TurnRequest::validate`.

### What did not change

Nothing about the **stream**: `fixtures/turn-stream.sse` is byte-identical to `2026-08-22`'s, and
so are `stream_events`, `output_items` and `usage_fields`. `prompt_cache_key`, the developer-message
head and `store: false` are unchanged. Everything in `2026-08-22`'s README still holds and is not
restated here.

## Fixtures

| File | Pins | Checked by |
| --- | --- | --- |
| `turn-request.json` | every field the harness sends, including **a tool choice naming the turn's own tool** | `crates/harness-responses/tests/contract.rs` builds it from the real projection and compares |
| `turn-stream.sse` | every event the harness interprets, framed as it arrives | the same test, through `decode_stream` |
| `manifest.json` | the event list and each fixture's digest | `scripts/check-provider-wires.py` |
| `manifest.json` `request_fields` | the exact field set the harness sends | the same Rust test |

## Conformance

`provider_emulated`. Every case runs against a deterministic local endpoint over a real socket.
Nothing here is evidence that a real provider behaves this way, including that it honours a tool
choice — that is documented by the vendor and unmeasured here.
