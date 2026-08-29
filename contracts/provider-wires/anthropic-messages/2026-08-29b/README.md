# `anthropic-messages` wire, version 2026-08-29b

The exact subset of the Anthropic Messages API this harness speaks. Immutable once released: a
change to what is sent or accepted opens a new dated version rather than editing this one.

**On the name.** The convention is a date, and a date alone cannot distinguish two cuts made on one
day. The second and any later cut on a date take a letter suffix, in order. Nothing else changes:
versions still sort chronologically, and `2026-08-29b` is read as *the cut after `2026-08-29`*.

## What changed from `2026-08-29`, and why

**One field, in one new place: a second `cache_control` breakpoint, on the last content block of
the last message.**

`2026-08-29` cached the constant head and nothing else — one breakpoint at the end of `system`,
which on this route's render order (`tools`, then `system`, then `messages`) covers everything
before the conversation. That version's own README called the uncached conversation a stated gap
waiting for a placement rule, and said the rule would come from an observation rather than an
argument. It has:

| observation | figure |
| --- | --- |
| Turns in the measured run | 81 |
| Cache hit rate, early in the run | 66% |
| Cache hit rate, late in the run | 12.5% |
| Input tokens spent, one state | 1.33M |
| Output tokens produced, same state | 10.5k |

That is the quadratic term. The loop is stateless, so turn *n* resends turn *n−1*'s transcript byte
for byte and adds to it; with the head cached and the tail not, every byte the conversation grows by
is paid for at the full input rate on every remaining turn.

The breakpoint **moves**. Each turn marks the current tail, so each turn writes the prefix it just
read and the next turn reads that back instead of paying for it again. One moving marker is enough
because this route looks for a hit at the breakpoint *and* at the blocks shortly before it, so the
previous turn's write is still found after a turn has appended a few blocks.

Two breakpoints are sent in total, against a documented cap of four. The remaining room is
deliberate: if a live run ever shows cache writes with no reads, a second marker held one turn
behind is the fix, and it fits.

### What the breakpoint is never placed on

A `cache_control` key **modifies** the block it lands on, and a `thinking` block carries a signature
the provider verifies against the block as the model produced it. Marking one would be AGENTS.md
invariant 5 broken and a turn the route rejects. So the marker is only ever placed on a block this
harness built itself — `text` or `tool_result` — and a conversation whose tail offers neither
carries no rolling marker at all. A missing breakpoint costs money; a modified opaque block costs
the turn.

### What did not change

Nothing about the **stream**: `fixtures/turn-stream.sse` is byte-identical to `2026-08-29`'s, and so
are `stream_events`, `content_block_deltas`, `output_items` and `usage_fields`. The request's field
**set** is unchanged too — `request_fields` is the same list — because the new marker is a key
inside `messages`, not a new top-level field. What moved is the bytes inside one message.

Everything else in `2026-08-29`'s README still holds and is not restated here: the endpoint, the
credential headers and their two routes, the disjoint usage figures, the item and delta handling
rules, the opaque-item rules, and what this contract does not pin.

## Prompt caching, stated whole

| where | breakpoint | why |
| --- | --- | --- |
| `system`, last block | fixed | covers `tools` + `system`, the constant head of every turn of a run |
| `messages`, last message's last markable block | **rolling** | makes the growth of the conversation cost once rather than once per remaining turn |

`usage.cache_creation_input_tokens` is read and carried as its own figure, and stays `None` when the
route does not report one — a route that never mentions cache writes has not said there were none.

## Fixtures

Both are load-bearing, and each is checked from a different direction.

| File | Pins | Checked by |
| --- | --- | --- |
| `turn-request.json` | every field the harness sends, on a turn carrying an instruction, a person's input, a replayed thinking block, one call, its result, all three sampling fields, and **both** cache breakpoints | `crates/harness-messages/tests/contract.rs` builds it from the real projection and compares |
| `turn-stream.sse` | every event the harness interprets, framed as it arrives on the wire, `event:` field and all | the same test replays it through `decode_stream`, the exact code a live turn uses |
| `manifest.json` | the event list, the delta list and each fixture's digest | `scripts/check-provider-wires.py` |
| `manifest.json` `request_fields` | the exact field set the harness sends | the same Rust test, not the Python checker |
| `manifest.json` `request_headers` | the header names each credential kind travels under | the same Rust test, against `header_names` — the function the client itself calls |

The stream fixture must decode with **no** warnings. A warning there means an event in the pinned
set is no longer recognized, which is drift the digest check alone would not catch.

## Conformance

`provider_emulated`. Every case runs against a deterministic local endpoint over a real socket. No
run against a real provider has happened here, and nothing in this directory may be read as evidence
that one behaves this way.

The cache figures above came from a **live run measured elsewhere** and are reported as the reason
for the change, not as conformance evidence for it. Whether these two breakpoints actually raise the
hit rate on a real route is unmeasured, and this contract does not claim it — it says these bytes
are sent, in these positions.
