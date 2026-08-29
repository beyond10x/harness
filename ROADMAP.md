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
pins the request, the stream and the credential headers, checked from both directions — **and is
superseded by `2026-08-29b`**, cut the same day for the rolling `cache_control` breakpoint that
caches the conversation and not only its head. `2026-08-29` stays as released (invariant 13); the
current pin is `2026-08-29b`. Reached.

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
difference while it was the only one. The copy was left standing on purpose for one release, so
that the second wire was the evidence rather than a guess acting on itself.

**Acted on: `crates/harness-http`.** That half is one crate beneath both wires, and each wire is now
its projection, its URL and its headers over `harness_http::HttpTransport` — neither depends on
`reqwest`. No behaviour moved with it: the two pinned contract suites, `check-provider-wires.py` and
both `provider_emulated` suites pass with no fixture, manifest or case edited. The extraction found
exactly **one** real difference between the two copies, and it is now named instead of implied —
the first route ends its stream with `data: [DONE]` and the second has no sentinel at all, so
`Framing` is a per-wire setting. Everything else was identical, including the status table: 529 was
already covered by the 5xx range on both sides and only the comments differed. What keeps them
honest is `crates/harness-messages/tests/transport.rs`, which compares whole settings values and
fails on any difference but the framing.

## Phase 4: subscription authentication

**Status: the source exists and the Anthropic route is authorized; renewal and the ChatGPT/Codex run do not.**

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
- **the ChatGPT/Codex authorized run.** That route has not been contacted. It needs no new code at
  all: it takes its access token as a plain bearer, which `StaticBearer` already does — what is
  missing is the run that says so;
- **a live contract version for the Anthropic route.** The run below happened; its *bytes* were not
  captured, so `contracts/provider-wires/anthropic-messages/2026-08-29b` — the current pin — is
  still `provider_emulated` and stays that way. Invariant 18 forbids promoting emulated evidence in
  place: a live pin is a **new dated version** cut from captured bytes, not an edit to this one.
  The cache-breakpoint placement `2026-08-29b` introduces is the part most worth capturing live:
  the measurement that argued for it is a hit-rate series, and the pin itself is emulated.

Done since, and it is the Anthropic half of this phase's exit:

- **one authorized run, 2026-08-29.** `b10x-harness run --wire anthropic-messages` against
  `https://api.anthropic.com/v1` on `claude-haiku-4-5-20251001`, reading a subscription token from a
  named file at a named JSON pointer: three turns, two tool calls, completed. The same route also ran
  end to end under `metaharness run b10x` and under `protocol drive`, which is what the flags were
  for;
- **the header shapes are discriminated against the route itself, not asserted.** A deliberately
  invalid token to the same endpoint answers `401 authentication_error`. Without that control the
  200 could be an endpoint indifferent to which header carried the credential, and the emulator
  cannot tell the difference.

**Exit:** one authorized run on each, with the credential never leaving the source that owns it.
Anthropic: met. ChatGPT/Codex: not met.

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

## Phase 7: what the loop owns beyond the catalogue

**Status: the first three landed 2026-08-29, `provider_emulated`; two stay out of scope.**

The comparison against other harnesses (`docs/reviews/2026-08-29-sota-comparison.md`, finding #13)
named five things every one of them has: sub-agents, structured output, hooks, an MCP client and
multimodal input. Design 0002 is the decision. `answer` and `delegate` are tools the **loop** owns
— resolved before the tool port sees a call, meeting the same gate, batched never — and a hook is
a port like the approver, with the process-running half in the shell. Each is opt-in per run.

**Exit evidence:** the `answer` path (call, nudge, `unstructured`), a delegate that reads and
reports, and each of the three hook points, all driven end to end over both emulators through the
shipped binary. Reached the same day. **What is not reached**, and what the next evidence is: one
live run per feature — how often a real model ends in prose under `answer` is the measurement that
decides whether provider-native constrained decoding (M2) is cut as new contract versions.

Out of scope, and why: an MCP client would make this loop a client of a protocol whose tools
nothing here confines — metaharness is the MCP side of this family; multimodal input is a new
neutral value on both wires that nothing measuring this harness has asked for.

## Phase 8: the workflow runner — the loop walks a workflow itself, with the governor outside

**Status: in progress — design 0003; M1 under `Unreleased`.**

`crates/harness-flow` is 1,891 lines and 27 tests: a DAG of sub-trees, a group as a context scope,
`Repeat` as the shape of a retreat, `gives` as the only thing that crosses a group boundary, and
`Flow::run` walking a validated plan against a caller's `StepRunner`. Every `StepRunner` that
exists is in its own `tests.rs`; no crate in `harness-cli` depends on it. On the other side of the
boundary, engineering-protocols already projects into it — `protocol workflow flow --id adp/default/2
--map …` emits `fixtures/adp-default.projected.yaml`, and that document plans and retreats here.
The projection says what it is: **an ordering, not a government.** Guards, the `declined` outcome
and every early exit are dropped, and the retreat bound is a number on the command line because the
source bounds a retreat with the engine's iteration budget.

**Why the runner has to live here, and not stay a process-per-step driver.** Today a workflow runs
this loop in exactly one way: `protocol drive run` in engineering-protocols spawns the binary once
per `llm` step, through `metaharness run b10x`, with the step's prompt, `--context` files,
`--write-scope` and `--allow-program`, and nothing else. The loop never sees the graph; every step
starts cold; a retreat is the engine re-entering a state and paying for the context again.
metaharness is the right spawner for a *vendor* harness — a scratch home, a copied plugin tree, a
hook channel, a retained transcript — and for this loop it adds an argv and an attestation, which its
own adapter says in as many words. Phase 5's consumer embeds this loop as a library. A driver that
is a process tree of `protocol drive` → `metaharness` → `b10x-harness` per step cannot be embedded,
and an embedder that wants a workflow wants its ordering, its context scope and its retreat *inside*
the loop it holds. So the runner is this component's, and it must need neither metaharness nor a
`protocol` process to walk a plan.

**What stays outside, by decision.** The governor. The engine (`aep-engine`: guards, evidence,
transitions, visit and attempt budgets) and the step map (`aep-driver-spec`) are
engineering-protocols', and they stay there: this harness embeds nothing above it (invariant 2), and
a driver that evaluated a gate would be a second protocol implementation with none of the
conformance suites behind it — engineering-protocols' own guide refuses that by name. The driver is
not in metaharness and nothing has to be extracted from it. What is worth taking apart is on the
engineering-protocols side: the routing core (`aep-driver`, 90 lines) is a library already; the
per-harness argv, the per-call `decide_tool`, store integrity and the run directory are the 6,994
lines of `protocol drive`. The bridge asks that repository for one new thing — a way to put **one
transition** to the engine from a run cursor, as a program the loop can call — and nothing else.

**The bridge is bytes, in both directions, over ports this loop already has:**

| leg | mechanism | owner |
|---|---|---|
| workflow in | the flow document, `protocol workflow flow --map <steps> --max-attempts N` | engineering-protocols, exists |
| step → turn | a `StepRunner` in `harness-cli`: one step is one turn in the scope's session, the handoff is the step's `answer` against the group's `gives` | here, absent |
| transition out | a fourth hook point on `--hooks`, `transition`: fires before a group is entered and after it leaves, carries flow id, path, attempt and handoff; a block is one more refusal, exactly as `before-call` is | here, absent |
| the governor | any program behind that hook — `protocol drive` answering one transition from its cursor, or nothing, in which case the run is ordered and not governed and its record says so | engineering-protocols, absent |
| the record | `flow.*` events on `--json`; metaharness maps each to an IR family or lists it as control plane, when an eval wants the run | metaharness, absent, optional |

**What this is not: an eval arm.** Under the three-arm program the workflow runs in the engine on
every arm, and the arms are comparable because only the treatment varies. A run under this phase
moves the sequencer, so it is a different experiment, not a fifth column of the same one. Where it
is measured against the driven native arm is cost, tokens and wall-time under the **same** governor
program — the warm-context claim above is a number to be produced, not a property to be asserted.

Steps, each its own story:

1. `StepRunner` bound to a turn: a group's steps share one session, a step in a new group starts
   from `available` and nothing else, `handoff` reads the structured `answer`. Both emulators.
2. `workflow run --flow <FILE> [--max-attempts N]`, and `flow-started`, `group-entered`,
   `step-started`, `step-finished`, `group-repeating`, `group-left`, `transition-refused`,
   `flow-finished` on `--json`, rendered on stderr like
   everything else.
3. The `transition` hook point, with the same *declared, never discovered; narrowing only* rules.
4. The metaharness projection of `flow.*`, only when an eval asks for it.

**Exit evidence:** the shipped binary walks `adp-default.projected.yaml` end to end over both
emulators, takes one retreat and stops at its bound, and puts every transition to a hook program
that refuses one of them — with no `metaharness` and no `protocol` process alive; and one embedded
run under Phase 5's consumer does the same through the library.
