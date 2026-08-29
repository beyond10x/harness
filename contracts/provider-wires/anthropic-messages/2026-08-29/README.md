# `anthropic-messages` wire, version 2026-08-29

The exact subset of the Anthropic Messages API this harness speaks. Immutable once released: a
change to what is sent or accepted opens a new dated version rather than editing this one.

The first version of the second wire, so there is nothing to diff against. What follows instead is
what differs from `openai-responses`, because that is where a reader's expectations come from.

## Shape

| | |
| --- | --- |
| Endpoint | `POST {base_url}/messages` |
| Transport | HTTPS with a server-sent-event response body |
| API version | `anthropic-version: 2023-06-01`, on every request |
| Credential | `x-api-key` for a key issued to a program; `authorization: Bearer` **plus** `anthropic-beta: oauth-2025-04-20` for a token obtained on a person's behalf |
| Stateful | No. The whole conversation is replayed each turn and nothing is retained on the far side |

## What differs from the first wire, and why each difference is load-bearing

| | `openai-responses` | `anthropic-messages` |
| --- | --- | --- |
| Transcript | a flat `input` array, one entry per item, role on the entry | `messages`, each with one role and a list of **content blocks**; consecutive items on one side merge into one message |
| A tool result | its own `function_call_output` entry | a `tool_result` block inside a **user** message, answering a `tool_use` block in an assistant one |
| Tool arguments | a JSON **string** | a JSON **object** |
| Reasoning | a `reasoning` output item, opaque | `thinking` / `redacted_thinking` **content blocks**, opaque, carrying a signature the provider verifies |
| Output bound | optional; absence is preserved | **required**. Absence cannot be preserved and resolves to the endpoint's own number |
| Effort | nested under `reasoning` | nested under `output_config` |
| Temperature | `0.0..=2.0` | `0.0..=1.0`, so a value the neutral layer admits is refused here by name |
| Tool names | `^[a-zA-Z0-9_-]+$` | the same class, **plus a 128-byte cap** |
| Usage | `input_tokens` is the whole and cached is a subset | the three input figures are **disjoint** and the projection sums them |
| Prompt caching | a `prompt_cache_key` keyed on the conversation | a `cache_control` breakpoint at the end of `system` |
| Terminal marker | a `[DONE]` framing sentinel | a `message_stop` **payload** |

Two of those are the reason `harness-wire` was widened, and the reason is recorded where each
widening landed:

- **`Usage::cache_creation_input_tokens`** — this route reports cache **writes** as their own
  figure and bills them above the plain input rate. Dropping it would have made a cache-writing
  turn indistinguishable from one that read nothing, and it is an `Option` because a route that
  never mentions cache writes has not said there were none.
- **`BearerSource::kind`** — the same secret travels under different header names on this
  endpoint's two routes, so which one a credential is stopped being derivable from the wire alone.
  The **kind** is neutral; the header names are here.

## Prompt caching

`system` is a **block list**, not a string, so it can carry `cache_control: {"type": "ephemeral"}`.
The render order this route caches over is `tools`, then `system`, then `messages`, so one
breakpoint at the end of `system` covers the whole constant head of every turn.

The **conversation is not cached**, and that is a stated gap rather than an oversight. The loop is
stateless: turn *n* resends turn *n−1*'s prefix byte for byte, so the cost of a run is quadratic in
its turns and a breakpoint on the growing tail is what makes it linear. Placing one needs a rule
this wire has no measurement for yet, and the first wire's own cache key was chosen from an
observation rather than an argument. This one will be too.

## Fixtures

Both are load-bearing, and each is checked from a different direction.

| File | Pins | Checked by |
| --- | --- | --- |
| `turn-request.json` | every field the harness sends, on a turn carrying an instruction, a person's input, a replayed thinking block, one call, its result, and all three sampling fields | `crates/harness-messages/tests/contract.rs` builds it from the real projection and compares |
| `turn-stream.sse` | every event the harness interprets, framed as it arrives on the wire, `event:` field and all | the same test replays it through `decode_stream`, the exact code a live turn uses |
| `manifest.json` | the event list, the delta list and each fixture's digest | `scripts/check-provider-wires.py` |
| `manifest.json` `request_fields` | the exact field set the harness sends | the same Rust test, not the Python checker |
| `manifest.json` `request_headers` | the header names each credential kind travels under | the same Rust test, against `header_names` — the function the client itself calls |

`content_block_deltas` is pinned separately from `stream_events` because on this route the
interesting variation is **inside** `content_block_delta`: `text_delta`, `input_json_delta`,
`thinking_delta` and `signature_delta` are four different things one outer event name covers, and
pinning the outer names alone would pin almost nothing.

The stream fixture must decode with **no** warnings. A warning there means an event in the pinned
set is no longer recognized, which is drift the digest check alone would not catch.

`output_items`, `usage_fields`, `endpoint`, `transport`, `streaming` and `stateful` are documented
here and in the manifest but **not** mechanically checked. Treat them as prose.

## Handling rules

- A content block outside `text`, `tool_use`, `thinking` and `redacted_thinking` is preserved as an
  opaque value and reported as a warning. Dropping it would leave a hole the next turn cannot see.
- A stream event outside the pinned set is reported as a warning and skipped, and so is a
  `content_block_delta` whose own type is outside the pinned set.
- `ping` is a keep-alive and is **not** reported as unknown.
- Streamed arguments that do not parse are a typed refusal, and so are arguments that parse to
  anything other than an object. A half-parsed argument blob must never reach a tool, because the
  tool would act on a value the model did not send.
- A thinking block is replayed **verbatim and in place**. Nothing reorders content blocks, which is
  what keeps a thinking block first in its message without this code having to know why that
  matters. Its signature is what the provider verifies; an edited block is a rejected turn.
- A thinking block from this wire replayed into another is a typed refusal naming both wires, and
  so is a `reasoning` item from the first wire replayed into this one (AGENTS.md invariant 5).
- A stream that ends before `message_stop` is a truncation, never a completion.
- Absent usage stays absent. A zero would be a claim that no tokens were spent.
- `stop_reason` values this crate does not model — `pause_turn`, `refusal`, `stop_sequence` — are
  carried under their own names. A refusal reported as a finished turn is a refused run a caller
  would write down as completed.
- An `overloaded_error` in the stream is the far side asking for less traffic and is retriable; any
  other stream error is a refusal and is not.

## The credential, and what this contract does not pin

The header names are pinned; **the token is not this contract's business**. It is fetched from an
injected source at call time, written into one header, and dropped. There is no ambient fallback:
the harness reads nothing it was not pointed at, and no fixture here contains a credential — the
emulator records which header carried one and its length, never its value (AGENTS.md invariant 17).

What is **not** implemented, and is stated so nobody reads its absence as working: this harness
does not **renew** a subscription token. It re-reads the named source on every call, so a token an
owner outside the process renews is picked up on the next turn; a token nobody renews expires and
the run fails by name.

## What this contract does not say

That any endpoint **acts** on these fields. It says they are sent, in these positions.

## Conformance

`provider_emulated`. Every case runs against a deterministic local endpoint over a real socket. No
run against a real provider has happened, and nothing here may be read as evidence that one behaves
this way.
