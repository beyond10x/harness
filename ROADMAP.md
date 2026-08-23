# Harness roadmap

An outcome roadmap. A phase advances only when its exit evidence exists; a compiling scaffold does
not stand in for behavioral proof.

## Phase 1: the loop, over one wire

**Status: complete.**

- neutral values and the three ports, with no I/O, clock, credential, or vendor field name;
- the Responses wire: streaming SSE, request projection, tool-call decode, reasoning preservation,
  usage, stop reasons, cancellation, typed HTTP status mapping;
- the loop: turn assembly, tool round trips, approvals, budgets it can actually count, refusal of
  one it cannot;
- a command line over a read-only workspace, so the whole thing is runnable by a person;
- a pinned wire contract checked from both directions.

**Exit evidence:** the built binary answers, calls a tool against a real file, and reports real
token counts, driven over a real socket against a deterministic local endpoint. Reached.

## Phase 2: bridge mode

**Status: implemented; the cross-component proof is open.**

A process speaking the Codex app-server JSON-RPC format, so `runtime/agent`'s existing bridge drives
this harness with no new bridge code — `AppServerChild::spawn` already takes an arbitrary command.

Done:

- the pinned client methods `initialize`, `initialized`, `thread/start`, `turn/start` and
  `turn/interrupt`, with `thread/resume` and `turn/steer` refused by name rather than answered with
  a silent success;
- the pinned server notifications, including `turn/started`, agent-message deltas, `item/started`,
  `item/completed`, `thread/tokenUsage/updated` and `turn/completed`;
- tools accepted as `dynamicTools` on `thread/start` and called back through `item/tool/call` — a
  second `ToolPort` implementation over the wire, with the loop unchanged;
- an interrupt acted on when its frame is decoded and acknowledged between streamed events, so a
  turn blocked on the model actually stops.

**Exit:** the existing bridge, pointed at this binary instead of `codex`, drives a turn. Everything
so far is this component's own client, written from the bridge's published source — the two
processes have never spoken, and `STATUS.md` says so rather than implying otherwise.

## Phase 3: the second wire

**Status: not started.**

`anthropic-messages` over `POST {base}/messages`. Same loop, same fixtures re-pointed; the work is
the projection, plus `thinking` blocks becoming opaque items.

**Exit:** both wires pass the same loop suite. If `harness-wire` needs widening to fit the second
wire, that widening is the evidence the first abstraction was wrong — it lands here, with the
reason recorded, rather than being guessed at in phase 1.

## Phase 4: subscription authentication

**Status: not started.**

ChatGPT/Codex and Claude subscription routes: OAuth plus per-route headers, as further
`BearerSource` implementations. Last, because they carry credential-custody questions an API key
does not.

**Exit:** one authorized run on each, with the credential never leaving the source that owns it.

## Phase 5: embedding and live characterization

**Status: not started.**

- a `runtime/agent` direct-provider adapter that embeds this loop and binds `ToolPort` to its
  capability compiler — the first consumer, and the first time the tools are real operations;
- one explicitly authorized live run against a real gateway, retained as `vendor_live` evidence
  distinct from everything above it.

**Exit:** a direct-provider run passes `runtime/agent`'s own lifecycle conformance, and a live run
exists whose evidence is not confused with provider emulation.

## Phase 6: `harness-workspace`, one trait over three ways to hold a tree

**Status: not started.** The shape is already visible in `harness-substrate::Backend`, which is
where the argument for pulling it out comes from.

A run's tools need a tree they may read and change. Today there are two implementations of that and
they live in two crates for historical reasons rather than for a reason: `WorkspaceTools` reads the
operator's own directory with no confinement at all, and `ConfinedTools` reaches substrate — either
embedded in this process or across a socket. A third is missing and obvious.

The three, and what each is for:

| implementation | confinement | who asked | for |
|---|---|---|---|
| **non-confined** | none: the process's own filesystem, bounded by path checks this crate makes | nobody — there is no boundary to name a subject at | a run against the operator's own tree, which is what every run so far has been |
| **substrate as a library** | the driver's: guarded IO, `openat2` containment, cgroups and namespaces around an exec | nobody: in-process there is no peer, so no subject | a simple run that wants real confinement and no deployment |
| **substrate over a socket** | the same, plus an authenticated boundary | a subject derived from kernel peer credentials | an integrated or multi-tenant deployment, where *who asked* has to be answerable |

What pulling it out buys: the publication gate stops being a property of one crate. Today
`ConfinedTools::new` decides what exists from `Facts`, and `WorkspaceTools` publishes three tools
unconditionally — two rules in two places for one question. One trait means the toolset is computed
once from what the chosen workspace admits, and an embedder that passes a remote gets the same
answer for the same reason.

**Exit:** `harness-cli` names a workspace implementation and publishes what it admits, with no
`cfg` and no branch on which one it got; the three implementations share one conformance suite; and
`ToolPort` has one implementation rather than two that must be kept agreeing.
