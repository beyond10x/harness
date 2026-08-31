---
title: Provider wires
description: The two model API projections, their shared transport, and continuation rules.
---

# Provider wires

Harness separates the agent loop from the provider API used for one model turn. The loop sees a
neutral `ModelPort`; a wire projects its request and decodes its stream.

## Supported projections

| Wire ID | Endpoint | Streaming | Selection |
|---|---|---|---|
| `openai-responses` | `POST {base-url}/responses` | SSE | default |
| `anthropic-messages` | `POST {base-url}/messages` | SSE | `--wire anthropic-messages` |

Both wires receive the same neutral conversation, tool definitions, sampling values, and output
bound. Below their projections they share the HTTP transport, bounded SSE framing, retry policy,
backoff, cancellation, and status mapping.

The framing differs in one explicit setting: the Responses stream ends with a `[DONE]` sentinel;
the Messages stream does not.

## Stateless continuation

Harness retains no provider-side thread ID. Each turn sends the complete local conversation again.
Sessions are therefore client-side files, not handles to state held by a model provider.

Reasoning items that are not part of the neutral value model are preserved as opaque values tagged
with the wire that created them. They can be replayed through that wire and nowhere else. A
cross-wire resume is a typed refusal rather than a silent drop.

## Credentials

Credential acquisition is separated from provider presentation. The caller names a source; the
selected wire constructs the request headers required by its route. Transport code receives a
finished URL, headers, body, and decoder and contains no vendor names or endpoint paths.

Use API-key sources for a normal bearer route. OAuth sources can select a token from a JSON
document and re-read files on every attempt so an external owner can rotate them. A built-in
provider may supply a documented OAuth path; the `codex` provider may renew and atomically rewrite
only the default it supplied before the first request. Explicitly named sources are never renewed
or written. See [Configuration reference](./configuration.md#the-credential-is-defaulted-and-the-record-says-so).

## Retry behaviour

The transport does not blindly resend a witnessed stream. Once output was observed, only the loop
knows whether the conversation is still unchanged and a retry is safe to attempt.

The loop retries an interrupted turn up to three times with cancellable backoff. It emits
`turn-retried` before the next attempt; consumers must discard all deltas from the interrupted
attempt. See [Sessions and events](../guides/sessions-and-events.md).

## Contracts and evidence

Provider-wire contracts pin exact request bytes, non-secret header values, terminal policy, and the
full accepted event inventory under `contracts/provider-wires/<wire>/<version>/`. The independent
`cargo xtask provider-contracts` validator checks each manifest, fixture, digest, inventory and
released-version immutability, while tests in the wire crates exercise the production encoder,
header builder and decoder against those same bytes.

Most current evidence comes from deterministic local provider emulators. It proves the projection,
transport, and loop scenarios against a real socket; it is not a claim that every live service
implementing a similarly named API behaves identically. See [Status and limitations](../status.md).

## App-server bridge

`b10x-harness app-server` exposes the same loop as one JSON-RPC connection over stdio. Tools arrive
from the client on `thread/start`; the endpoint and credential stay outside the protocol.

The served profile is the experimental dynamic-operation-tools profile because the stable
`codex-app-server-stdio-v2` profile admits no dynamic tools. A client must negotiate its experimental
API before registering tools. `thread/resume` and `turn/steer` are currently refused by name.
