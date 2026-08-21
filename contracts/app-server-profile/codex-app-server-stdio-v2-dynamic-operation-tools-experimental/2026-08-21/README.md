# `codex-app-server-stdio-v2-dynamic-operation-tools-experimental` profile, 2026-08-21

The subset of the Codex app-server JSON-RPC format this harness **serves**. Pinned after
`codex-cli 0.145.0`, the version `runtime/agent` drives.

Immutable once released. Widening what the server accepts or emits opens a new dated version.

## Why this profile and not the plain one

An earlier revision declared `codex-app-server-stdio-v2`. That is the client's **stable** profile:
it registers no dynamic tools, refuses `item/tool/call` as an out-of-profile server method, and
cannot classify a `dynamicToolCall` item at all. A server declaring it while emitting tool frames
looks compatible and fails at the first tool call. The operation-tools profile is the one that
carries generic Daemonloom operation tools, which is exactly the intended consumer.

Its price is a capability handshake: the client must send `capabilities.experimentalApi` at
`initialize`. Registering tools without it is refused by name at `thread/start`, while a text-only
thread works either way.

## Why speak someone else's protocol

`runtime/agent` already knows how to drive a process speaking this format, and the command it spawns
is arbitrary — `AppServerChild::spawn` takes a `Command`. Speaking the format means that investment
drives this harness with **no new bridge code**, and without either component depending on the
other. A protocol is the seam; a shared crate would have been a coupling.

## What this server is

| | |
| --- | --- |
| Product | `daemonloom-harness` — never `codex-cli` |
| Transport | JSON lines on stdio, one frame per line, flushed per frame |
| Frame bound | 8 MiB, matching the client's `MAX_LINE_BYTES`, capped at the read |
| Tool answer bound | 256 KiB, matching the client's `MAX_DYNAMIC_TOOL_RESPONSE_BYTES` |
| Endpoint and credential | outside the protocol, on the command line |
| Tools | supplied by the client as `dynamicTools` on `thread/start` |
| Approval | the client's, on its side of the callback |

`initialize` reports the product name so a reader can tell which implementation answered. Note the
pinned client **discards** that response, so it is for a person reading a transcript, not a
negotiation.

## Methods

**Served:** `initialize`, `initialized`, `thread/start`, `turn/start`, `turn/interrupt`.

**Refused by name:** `thread/resume` (nothing is retained after a turn) and `turn/steer` (the loop
cannot yet redirect a turn in flight). Both answer `-32601`. A client told a thread resumed, or a
turn was steered, would carry on believing something happened that did not.

**Emitted:** `thread/started`, `turn/started`, `item/started`, `item/tool/call`, `item/completed`,
`item/agentMessage/delta`, `thread/tokenUsage/updated`, `turn/completed`.

## Orderings that are load-bearing

- `item/started` carrying a `dynamicToolCall` item comes **before** `item/tool/call` for that call.
  The client registers the call from the first and refuses a callback for one it has not seen.
- `item/completed` for the call comes **after** the client answers it.
- `turn/completed` is last, and its status is one of `completed`, `failed`, `interrupted` — the
  three the client accepts. A run stopped by a budget is `failed`, not `completed`: the model did
  not finish, and a client told otherwise would treat a truncated run as an answer.
- An `agentMessage` `item/completed` is emitted only for a turn that actually completed. A failed or
  interrupted turn must not also deliver an answer.

## Control while a turn is running

`turn/interrupt` is acted on the instant its frame is decoded, on the reading thread, and
acknowledged between streamed events. Both halves matter: a turn spends most of its time blocked on
the model, so a server that only looked at its input between messages would acknowledge an interrupt
after the turn it was meant to stop had already finished.

An interrupt that was actually requested is reported as `interrupted` even if the connection then
drops. A connection that simply dies mid-turn is `failed`, with the reason. Collapsing the two would
report a person's own cancellation as a fault.

## Fixtures

| File | Pins | Checked by |
| --- | --- | --- |
| `walking-trace.jsonl` | two turns over one connection: handshake with the capability, a refused `thread/resume`, a thread with a registered operation tool, a tool round trip, a streamed answer, a completed terminal, then a second turn carrying a refused `turn/steer`, an interrupt, and an `interrupted` terminal | `scripts/check-app-server-profile.py` — every frame is a declared method, every request is answered, and **every declared method on both sides** appears |
| `manifest.json` | the three method lists, the terminal statuses, the tool item type, and each fixture's digest | the same script, plus `crates/harness-app-server/tests/contract.rs`, which checks the server's own constants against it |

## What these checks do **not** catch

The trace pins method names, not frame contents. Removing a required field such as `turnId` from a
notification would leave both checks green and still be rejected by the real bridge. Nothing here
compares this server's inventory to the client's — the no-dependency rule forbids reading it — so a
mismatch after a Codex version bump is caught only by review, or by running the real bridge.

## Conformance

`provider_emulated`. The suite in `crates/harness-cli/tests/bridge_mode.rs` drives the shipped binary
as a client over real pipes, against a deterministic local model endpoint.

**The real bridge has not driven this server.** That crosses a component boundary and has not been
run. Until it has, this profile is evidence that the frames match what the client's own source says
it sends and expects — not evidence that the two processes have ever spoken.
