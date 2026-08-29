# `anthropic-messages` wire, version 2026-08-30

The exact subset of the Anthropic Messages API this harness speaks. Immutable once released: a
change to what is sent or accepted opens a new dated version rather than editing this one.

## What changed from `2026-08-29b`, and why

**One new top-level request field: `tool_choice`.**

A run under an output schema finishes by calling that schema's tool. A model that ends in prose
instead is told once to call it and, if it still does not, the run stops `unstructured` — every
turn paid for and nothing reported. `ROADMAP.md` Phase 7 said the measurement would decide whether
provider-native constrained decoding was worth a version. The measurement:

| observation | figure |
| --- | --- |
| Paid native walk | 2026-08-30, `metaharness` `native-eval.hUbOP5`, Haiku 4.5 |
| Attempts at one section that ended in prose under the nudge alone | **3 of 4** |
| Sections that produced nothing as a result | 1 of 2 reached |

So the nudge is now asked twice: once in words, once as this route's own constraint. The field is
sent on **at most one turn per run** — the turn a nudge opens — and never on any other.

| neutral value | this route sends |
| --- | --- |
| `auto` | *nothing.* The model choosing is this route's default, and sending `auto` would make its choice ours |
| `required` | `{"type": "any"}` |
| named | `{"type": "tool", "name": "<the tool>"}` |

A turn held to a tool it does not publish is refused before it is sent, by
`TurnRequest::validate` — the two lists are in hand there, and the far side's rejection names only
its own field.

### What this costs

This route renders `tools`, then `system`, then `messages`, and the cache breakpoints sit at the
end of `system` and on the conversation's tail. `tool_choice` is outside both, so a turn that
carries it may not be served from the cached prefix. That is one turn per run, on a run that was
otherwise about to report nothing — which is the whole reason the loop holds one turn and not
every turn.

### What did not change

Nothing about the **stream**: `fixtures/turn-stream.sse` is byte-identical to `2026-08-29b`'s, and
so are `stream_events`, `content_block_deltas`, `output_items` and `usage_fields`. Both cache
breakpoints are still sent, in the same two places, and are still never placed on an opaque
`thinking` block. Everything in `2026-08-29b`'s and `2026-08-29`'s READMEs still holds and is not
restated here.

## Fixtures

Both are load-bearing, and each is checked from a different direction.

| File | Pins | Checked by |
| --- | --- | --- |
| `turn-request.json` | every field the harness sends, on a turn carrying an instruction, a person's input, a replayed thinking block, one call, its result, all three sampling fields, both cache breakpoints and **a tool choice naming the turn's own tool** | `crates/harness-messages/tests/contract.rs` builds it from the real projection and compares |
| `turn-stream.sse` | every event the harness interprets, framed as it arrives on the wire, `event:` field and all | the same test replays it through `decode_stream`, the exact code a live turn uses |
| `manifest.json` | the event list, the delta list and each fixture's digest | `scripts/check-provider-wires.py` |
| `manifest.json` `request_fields` | the exact field set the harness sends | the same Rust test, not the Python checker |
| `manifest.json` `request_headers` | the header names each credential kind travels under | the same Rust test, against `header_names` — the function the client itself calls |

The fixture holds a **named** choice because that is the only shape the harness ever sends: `auto`
is absence, and nothing in this harness asks for `any`. What `required` projects to is pinned by a
unit test rather than by these bytes.

## Conformance

`provider_emulated`. Every case runs against a deterministic local endpoint over a real socket. No
run against a real provider has happened here, and nothing in this directory may be read as
evidence that one behaves this way — including that this route honours a tool choice, which is
documented by the vendor and unmeasured here.
