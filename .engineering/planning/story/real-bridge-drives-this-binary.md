---
format: aep.planning-md/1
id: story:real-bridge-drives-this-binary
kind: story
status: draft
title: The bridge that was written against this protocol drives this binary
relations:
- derived_from: epic:bridge-mode-proof
revision: 2
---
## Evidence

- `STATUS.md:17` — "**run `runtime/agent`'s real bridge against this binary.** Everything so far is this component's own client, written from the bridge's source; the two processes have never spoken, and no gate compares the two inventories".
- `ROADMAP.md:26-28` — "A process speaking the Codex app-server JSON-RPC format, so `runtime/agent`'s existing bridge drives this harness with no new bridge code — `AppServerChild::spawn` already takes an arbitrary command."
- `ROADMAP.md:42-44` — the exit: "the existing bridge, pointed at this binary instead of `codex`, drives a turn".
- `AGENTS.md:105-109` — invariant 15: the inventory here is a copy, nothing checks the copy, "and the only thing that catches a mismatch is running the real bridge".
- `AGENTS.md:110-113` — invariant 16: declaring the client's *stable* profile while emitting `item/tool/call` "yields a server that looks compatible and fails at the first tool call".
- `STATUS.md:17` — what is already served: `initialize`, `initialized`, `thread/start`, `turn/start`, `turn/interrupt`, with `thread/resume` and `turn/steer` refused by name.
- `crates/harness-cli/tests/bridge_mode.rs` — 17 tests, all driving this repository's own client (`STATUS.md:61`).

## Context

Bridge mode is the one interface here whose counterpart is another program rather than a provider,
and by invariant 2 this repository may not import that program to check itself against it. So the
proof is a run, and it is the last one nobody has done.

The failure it is looking for is specific and quiet: a profile or an inventory that looks compatible
through the handshake and fails at the first tool call. A text-only turn would not find it.

## Acceptance

`runtime/agent`'s existing bridge, with its command pointed at `b10x-harness app-server`, completes a
turn that includes at least one tool call and one interrupt, and the run is recorded with the client
version it used.
