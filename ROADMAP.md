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

**Status: complete.**

`anthropic-messages` over `POST {base}/messages`. Same loop, same fixtures re-pointed; the work was
the projection, plus `thinking` blocks becoming opaque items.

**Exit evidence:** both wires pass the same 20-case loop suite against a real socket — the same case
names over the same scenario names, with `the_two_wires_serve_the_same_scenarios` failing if either
side grows a case the other lacks. The shipped binary drives either one on a `--wire` flag and the
loop below it cannot tell which it got. `contracts/provider-wires/anthropic-messages/2026-08-29`
pins the request, the stream and the credential headers, checked from both directions. Reached.

`harness-wire` needed widening twice, and each widening carries its reason where it lands:

- **`Usage::cache_creation_input_tokens`.** The second route bills cache *writes* as their own
  class. It is an `Option` because a route that never mentions cache writes has not said there were
  none. `Usage` now also states out loud what it had only ever implied — `input_tokens` is the whole
  and the cache figures are parts of it — because the second route reports its three input figures
  **disjointly** and something had to reconcile the two. The projection sums them; a value whose
  meaning depended on which wire produced it would make every figure downstream ambiguous.
- **`BearerSource::kind`.** One endpoint, two routes, the same secret under **different header
  names**. The first wire never needed to know what kind of credential it held because there was
  only one answer. The kind is neutral; the header names stay in the wire crate.

**And one thing the second wire proved wrong that is not in `harness-wire` at all.** Everything
between the HTTP client and the projection — bounded SSE framing, the retry rule, the witnessed sink
that makes the retry rule safe, the back-off, the status mapping — was copied unchanged, because
none of it is vendor-shaped. It is *transport*-shaped, and the first wire could not tell the
difference while it was the only one. A `harness-http` beneath both wires is the next structural
move; it was deliberately not made here, so that this change is the evidence rather than a guess
acting on itself.

## Phase 4: subscription authentication

**Status: the source exists; renewal and the authorized runs do not.**

ChatGPT/Codex and Claude subscription routes: OAuth plus per-route headers, as further
`BearerSource` implementations. Last, because they carry credential-custody questions an API key
does not.

Done:

- `harness_credential::SubscriptionToken`, a `BearerSource` that reads a token from a file or an
  environment variable the caller **names**, optionally at a caller-named JSON pointer. No default
  path, no vendor directory, no fallback when the named source is missing — the harness reads
  nothing it was not pointed at, and a source that searched on failure would be an ambient
  credential fallback whichever way it was spelled;
- re-read on **every** call rather than cached at construction, which is the whole of the renewal
  story here: an owner outside this process that renews the token is followed on the next turn;
- per-route presentation, keyed off the neutral `CredentialKind`: `authorization: Bearer` plus
  `anthropic-beta: oauth-2025-04-20` for a subscription token, `x-api-key` for a key issued to a
  program. The header names are pinned in the Messages contract and checked against the function the
  client itself calls;
- `--oauth-token-file` / `--oauth-token-env` / `--oauth-token-pointer`, mutually exclusive with the
  API-key flags.

Not done, and stated so nobody reads the absence as working:

- **renewal.** Nothing here holds a refresh token or calls an authorization server. A token nobody
  renews expires and the run fails by name;
- **the authorized runs.** No subscription route has been contacted. The Anthropic header shapes are
  `provider_emulated` — a deterministic local endpoint records *which header carried a credential*
  and its length, never its value. The ChatGPT/Codex half needs no new code at all: that route takes
  its access token as a plain bearer, which `StaticBearer` already does.

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

**Status: done, under a different name.** The trait is `harness_tools::Operations` and the crate is
`harness-tools`; it arrived as part of the one-tool-surface work rather than on its own, because the
same question — *what does this run admit?* — had to be answered once for the b10x loop and for the
MCP server metaharness serves to Claude Code.

What landed against the exit criteria below: `harness-cli` builds a `Catalogue` from whatever
provider it was handed and publishes what that admits, with no branch on which one it got; the
publication gate lives in `Catalogue::of` alone; and `ToolPort` has one implementation,
`harness_tools::Verbs`. The third implementation — substrate over a socket — exists as
`harness-substrate::Client` behind the same `ConfinedOperations`, so it is a deployment choice and
not a different set of things the model may do. What is **not** done is the shared conformance
suite: each provider has its own tests, and nothing runs one suite against all three.

The original text follows, since it is what the shape was argued from.

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
