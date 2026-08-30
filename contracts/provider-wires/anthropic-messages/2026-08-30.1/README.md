# `anthropic-messages` wire, version 2026-08-30.1

The exact subset of the Anthropic Messages API this harness speaks. Immutable once released: a
change to what is sent or accepted opens a new dated version rather than editing this one.

## What changed from `2026-08-30`, and why

**The request body now depends on how the credential is presented.** Under a subscription token
(`CredentialKind::Oauth`) `system` opens with a fixed block:

```json
"system": [
  {"type": "text", "text": "You are Claude Code, Anthropic's official CLI for Claude."},
  {"type": "text", "text": "<the run's own instruction>", "cache_control": {"type": "ephemeral"}}
]
```

A key issued to a program sends what it always sent — one block, the instruction, with the
breakpoint on it. That is why this version pins **two** request fixtures: `turn-request.json` is
the key shape and `turn-request-oauth.json` the subscription one. Both are built by the same
function in `crates/harness-messages/tests/contract.rs`; a second builder would prove only that the
second builder works.

## What was measured

2026-08-30, against `https://api.anthropic.com/v1` on the operator's own subscription token. Every
row is one request, `max_tokens: 1`, one user message:

| `system` sent | `claude-opus-5` |
| --- | --- |
| absent | `429` |
| `"You are a helpful assistant."` | `429` |
| the preamble **merged** into one block with other text | `429` |
| the preamble as block 0, further blocks after it | **`200`** |
| the preamble minus its trailing full stop | `429` |
| the preamble with a leading space | `429` |
| the preamble with a trailing newline | `429` |

`claude-sonnet-5` behaves as `claude-opus-5`. `claude-haiku-4-5-20251001` answers `200` under every
row, including the first — it is not gated. The `anthropic-beta` value makes no difference:
`oauth-2025-04-20` alone and `oauth-2025-04-20,claude-code-20250219` behave identically.

The match is therefore **exact and positional**, and both halves matter: the string must be
byte-identical, and it must be its own block.

## Why this is worth a version rather than a note

The refusal is `429 rate_limit_error` with the body `{"type":"error","error":{"type":
"rate_limit_error","message":"Error"}}` and **no `anthropic-ratelimit-*` headers at all**. A
successful request on the same token carries the full unified set, so the missing headers are
specific to this refusal.

Nothing downstream can tell that apart from an exhausted quota. The transport's own rule —
`RateLimited` is retriable — then spends four attempts and a back-off on a request that will never
be served, and reports a rate limit against an account measured at 8% of its five-hour window.

## The breakpoint moved

It is on the **last** `system` block, which is the instruction under either presentation. A
breakpoint covers everything rendered before it, so one at the end of `system` still covers the
whole constant head; put on the preamble instead it would leave the instruction — the larger half,
re-sent on every turn of a stateless loop — outside the cache.

## Conformance

`provider_emulated` for the fixtures, as before: the bytes are this projection's own output. The
gating rule above is **not** emulated — it is the seven rows in the table, each one a live request
to the vendor.
