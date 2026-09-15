# Harness roadmap

Serves **O1**, **O3** and **O6** of `atlas/ROADMAP.md`, the collection's objectives; this page orders the work inside this repository.

An outcome roadmap. A phase advances only when its exit evidence exists; a compiling scaffold does
not stand in for behavioral proof.

**Read against the `0.12.1` tree (2026-09-11).** Every status line below is what that tree shows.
No phase is in progress at this commit: `aep plan artifact list --store .engineering/planning
--status active` returns nothing, and the open work is held as draft artifacts under the epics each
phase names. `STATUS.md` carries the per-area state and the next piece of evidence each area waits
for; this page carries the ordering.

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
caches the conversation and not only its head. Every released version stays as released
(invariant 13), so the directory now holds `2026-08-29`, `2026-08-29b`, `2026-08-30`, `2026-08-30.1`
and `2026-08-31`; **the current Messages pin is `2026-08-31`**, and the current Responses pin is
`2026-08-31.1` (`STATUS.md`, *Wire contract*). Reached.

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
`reqwest`. No behaviour moved with it: the two pinned contract suites, the provider contract checker and
both `provider_emulated` suites pass with no fixture, manifest or case edited. The extraction found
exactly **one** real difference between the two copies, and it is now named instead of implied —
the first route ends its stream with `data: [DONE]` and the second has no sentinel at all, so
`Framing` is a per-wire setting. Everything else was identical, including the status table: 529 was
already covered by the 5xx range on both sides and only the comments differed. What keeps them
honest is `crates/harness-messages/tests/transport.rs`, which compares whole settings values and
fails on any difference but the framing.

## Phase 4: subscription authentication

**Status: done. Both routes are authorized, and one of them renews its own credential.**

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
  story *for that type*: an owner outside this process that renews the token is followed on the next
  turn. Renewing one is a separate function a caller invokes before the run, never something a
  bearer source does quietly mid-turn;
- per-route presentation, keyed off the neutral `CredentialKind`: `authorization: Bearer` plus
  `anthropic-beta: oauth-2025-04-20` for a subscription token, `x-api-key` for a key issued to a
  program. The header names are pinned in the Messages contract and checked against the function the
  client itself calls;
- `--oauth-token-file` / `--oauth-token-env` / `--oauth-token-pointer`, mutually exclusive with the
  API-key flags.

Not done, and stated so nobody reads the absence as working:

- **mid-run renewal.** The check below happens once, before the first request. A run that starts
  with a fresh token and outlives it still fails by name partway through, which is the case
  `story:oauth-token-renewal` is titled after;
- **a live contract version for the Anthropic route.** The run below happened; its *bytes* were not
  captured, so `contracts/provider-wires/anthropic-messages/2026-08-31` — the current pin — is
  still `provider_emulated`, as is every version before it, and stays that way. Invariant 18 forbids
  promoting emulated evidence in place: a live pin is a **new dated version** cut from captured
  bytes, not an edit to this one. The cache-breakpoint placement `2026-08-29b` introduced is the
  part most worth capturing live: the measurement that argued for it is a hit-rate series, and the
  pin itself is emulated.

Done since, and it is the Anthropic half of this phase's exit:

- **one authorized run, 2026-08-29.** `b10x-harness run --wire anthropic-messages` against
  `https://api.anthropic.com/v1` on `claude-haiku-4-5-20251001`, reading a subscription token from a
  named file at a named JSON pointer: three turns, two tool calls, completed. The same route also ran
  end to end under `metaharness run b10x` and under `aep drive`, which is what the flags were
  for;
- **the header shapes are discriminated against the route itself, not asserted.** A deliberately
  invalid token to the same endpoint answers `401 authentication_error`. Without that control the
  200 could be an endpoint indifferent to which header carried the credential, and the emulator
  cannot tell the difference.

And the ChatGPT/Codex half, which closes the phase:

- **one authorized run, 2026-08-30.** Two turns against `https://chatgpt.com/backend-api/codex` on
  `gpt-5.6-sol`, the token read from a named file at a named pointer (`~/.codex/auth.json`,
  `/tokens/access_token`): `file_read` called and answered, `finished{completed}`, session
  `18d066fc428e5e98-0003a176`. Same control as above — an unparseable token to the same endpoint
  answers `401 unauthorized_unknown` — so the success is the credential's and not the endpoint's
  indifference. `ROADMAP` predicted this needed no new code, and it did not;
- **a live refresh, 2026-08-29T23:26Z.** The renewal was run against the operator's own credential:
  `credential-renewed{expires_unix: 1788909974, refresh_token_rotated: true, byte_preserving: true}`,
  all three tokens rotated, 4 of 11 lines of `~/.codex/auth.json` changed with its key order, mode
  and unread keys intact, and `codex` itself still authenticating against the file afterwards;
- **a `codex` provider, and renewal.** Every value in it was read off that run. Unlike any provider
  before it, it carries a token endpoint, a client id and a pointer to the refresh token, so a run
  whose token is within fifteen minutes of expiring renews it and **writes the new one back**. That
  is the harness editing a file another program owns, and the bound is that it happens only for a
  credential the provider itself defaulted — a source the operator typed is read and never
  written — that `providers show codex` prints the file, the endpoint and the client before
  anything is spent, and that a run which renewed says so in the record with no part of the
  credential in it.

**Exit:** one authorized run on each, with the credential never leaving the source that owns it.
**Both met.**

## Phase 5: embedding and live characterization

**Status: begun — the first embedder exists; the binding and the live pin are open.**

The first consumer is **agent-platform**, not `runtime/agent`. `agent-platform/Cargo.toml:40-42`
pins `b10x-harness-wire`, `b10x-harness-loop` and `b10x-harness-messages` at tag **`0.10.0`**, and
`atlas/ROADMAP.md` records `agent-platform-harness` proving a compiled tool round trip through the
embedded loop. So *something outside this repository holds this loop as a library* is reached, at a
release two behind the current one. `STATUS.md`'s "No production component embeds it at this
commit" is the statement this page supersedes; it was true when it was written and is not true of
`0.10.0` onwards.

What remains, and what closes the phase:

- **the per-turn seam is pinned but not bound.** `TurnEnvironmentProvider` exists here and refreshes
  attributable context and a fail-closed tool subset before every model turn; the embedder has not
  implemented it, so per-turn revision evidence does not exist yet. Binding it is the embedder's
  work and the retained evidence is the embedder's to produce;
- **the embedder is two releases behind.** The cheapest close is a re-pin of agent-platform from
  `0.10.0` to the current tag, so a bug fixed here is a bug fixed there;
- **one explicitly authorized live run against a real gateway**, retained as `vendor_live` evidence
  distinct from everything above it. Every run in Phase 4 was against a vendor's own endpoint under
  the operator's own subscription, which is not the same thing.

**Exit:** an embedder binds `TurnEnvironmentProvider` and retains per-turn revision evidence, and a
live run exists whose evidence is not confused with provider emulation.

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
not a different set of things the model may do. The shared conformance suite is
`crates/harness-substrate/tests/conformance.rs`: 34 cases asked of all three implementations, run
from `cargo xtask gate` as its own named step. **All three exit conditions are now met**; the
missing-parent case is now a uniform named refusal across all three implementations.

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

**Status: four landed; multimodal input stays out of scope.**

The comparison against other harnesses (`docs/reviews/2026-08-29-sota-comparison.md`, finding #13)
named five things every one of them has: sub-agents, structured output, hooks, an MCP client and
multimodal input. Design 0002 is the decision. `answer` and `delegate` are tools the **loop** owns
— resolved before the tool port sees a call, meeting the same gate, batched never — and a hook is
a port like the approver, with the process-running half in the shell. Each is opt-in per run.

**Exit evidence:** the `answer` path (call, nudge, `unstructured`), a delegate that reads and
reports, and each of the three hook points, all driven end to end over both emulators through the
shipped binary. Reached the same day.

**The measurement happened, and it decided.** The seventh paid native walk (2026-08-30, Haiku 4.5,
metaharness `native-eval.hUbOP5`) ended in prose on three of four attempts at one section under the
nudge alone. So provider-native constrained decoding is cut: `TurnRequest::tool_choice`, projected
by both wires, sent on the turn the nudge opens and no other, pinned as
`anthropic-messages/2026-08-30` and `openai-responses/2026-08-30`. What is still not reached is a
live run per feature — everything above is `provider_emulated`, and whether either route honours a
tool choice is the vendor's documentation, not this repository's evidence.

**And a second measurement decided the other half of M4.** A run that asked for three sub-tasks in
one turn paid three whole child runs of latency back to back, because delegates ran strictly in
order — which is what the milestone said would be revisited *when a run shows the need*. It did, so
neighbouring `delegate` calls of one turn now run side by side (2026-08-30): `ModelPort::fork` and
`ToolPort::fork`, both answering *cannot* by default, hand each child its own model and tool port,
while the approver, the operator's hooks and the record stay single and are asked from the run's
own thread. Where a port will not fork or the token remainder will not divide, the same delegates
run in order — concurrency changes how long a turn takes and nothing else about what a run can do.
Delegate **trees** remain out: each level is a context nobody can read afterwards, and that
argument is untouched by this.

Outbound MCP landed on 2026-09-02 under design 0005. The ownership answer changed because the
protocol mechanics now live in a lower reusable `mcp` repository while this component keeps the
thing that matters to a harness: a local profile must pin registry and discovery digests and assign
every published name, description, envelope and subject. Discovery itself grants nothing, the list
is frozen before turn one, and calls traverse the ordinary approver and hook path. This is distinct
from metaharness, which still owns driving somebody else's loop over MCP.

Multimodal input remains out of scope: it is a new neutral value on both provider wires that nothing
measuring this harness has asked for.

## Phase 8: the workflow runner — the loop walks a workflow itself, with the governor outside

**Status: the binary half shipped; nothing is in progress.** Design 0003. M1 shipped in `0.2.0`;
of M2, command and operator steps landed — a `kind: command` step is one `run` call through the
run's gate, no model turn (2026-08-30), and a `kind: operator` step is a typed successful pause with
terminal `flow-paused`, no provider call and no invented failure or downstream skip (2026-08-31).
The last commit touching `crates/harness-flow` or `crates/harness-cli/src/workflow.rs` is
`a4e1218` (2026-08-31); everything since is MCP, documentation and releases. **Two items stay
open and neither is being worked at this commit**: flow resume (`--resume` is refused by name,
`crates/harness-cli/src/workflow.rs:426-431`, because a flow names one session per section) and the
library walk. Both are held as draft artifacts — `epic:embedded-by-a-consumer` and
`story:workflow-run-through-the-library` — and the store has no active artifact.

`crates/harness-flow` is a DAG of sub-trees, a group as a context scope, `Repeat` as the shape of a
retreat, `gives` as the only thing that crosses a group boundary, and `Flow::run` walking a
validated plan against a caller's `StepRunner`. The production `StepRunner` is
`crates/harness-cli/src/workflow.rs` — `FlowRunner`, which binds a step to one `AgentLoop::run_in`
over the same `Prepared` the `run` verb builds; the crate's own `tests.rs` holds the rest. On the
other side of the boundary, AEP already projects into it — `aep govern workflow flow --id adp/default/2
--map …` emits `fixtures/adp-default.projected.yaml`, and that document plans and retreats here.
The projection says what it is: **an ordering, not a government.** Guards, the `declined` outcome
and every early exit are dropped, and the retreat bound is a number on the command line because the
source bounds a retreat with the engine's iteration budget.

**Why the runner has to live here, and not stay a process-per-step driver.** The other way a
workflow runs this loop is one process per step: `metaharness aep drive` — the CLI atlas ADR 0047
fixes, with AEP's own model-backed invocations refusing before effects — spawns the binary once per
`llm` step, through `metaharness run b10x`, with the step's prompt, `--context` files,
`--write-scope` and `--allow-program`, and nothing else. The loop never sees the graph; every step
starts cold; a retreat is the engine re-entering a state and paying for the context again.
metaharness is the right spawner for a *vendor* harness — a scratch home, a copied plugin tree, a
hook channel, a retained transcript — and for this loop it adds an argv and an attestation, which its
own adapter says in as many words. Phase 5's embedder holds this loop as a library. A driver that
is a process tree of `metaharness aep drive` → `metaharness run b10x` → `b10x-harness` per step
cannot be embedded, and an embedder that wants a workflow wants its ordering, its context scope and
its retreat *inside* the loop it holds. So the runner is this component's, and it needs neither
metaharness nor an AEP process to walk a plan — which is what `workflow run` does today.

**What stays outside, by decision.** The governor. The engine (`aep-engine`: guards, evidence,
transitions, visit and attempt budgets) and the step map (`aep-driver-spec`) are
AEP', and they stay there: this harness embeds nothing above it (invariant 2), and
a driver that evaluated a gate would be a second protocol implementation with none of the
conformance suites behind it — AEP' own guide refuses that by name. The driver is
not in metaharness and nothing has to be extracted from it. What is worth taking apart is on the
AEP side: the routing core (`aep-driver`, 90 lines) is a library already; the
per-harness argv, the per-call `decide_tool`, store integrity and the run directory are the 6,994
lines behind `metaharness aep drive`. The bridge asks that repository for one new thing — a way to
put **one transition** to the engine from a run cursor, as a program the loop can call — and nothing
else.

**The bridge is bytes, in both directions, over ports this loop already has:**

| leg | mechanism | owner |
|---|---|---|
| workflow in | the flow document, `aep govern workflow flow --map <steps> --max-attempts N` | AEP, exists |
| step → turn | `FlowRunner`, the `StepRunner` in `crates/harness-cli/src/workflow.rs`: one step is one turn in the scope's session, the handoff is the step's `answer` against the group's `gives`, and a `kind: command` step is one gated `run` call instead | here, **shipped** |
| transition out | the fourth hook point on `--hooks`, `transition` (`crates/harness-cli/src/hooks.rs:216`, `:352`): fires before a group is entered and after it leaves, carries flow id, path, attempt and handoff; a block is one more refusal, exactly as `before-call` is, and a hook that cannot answer is read closed at both moments | here, **shipped** |
| the governor | any program behind that hook — `metaharness aep drive` answering one transition from its cursor, or nothing, in which case the run is ordered and not governed and its record says so | AEP, absent |
| the record | `flow.*` events on `--json`; metaharness maps each to an IR family or lists it as control plane, when an eval wants the run | metaharness, absent, optional |
| flow resume | a cursor a stopped walk can be re-entered from. `--resume` is refused by name today, because a flow has one session per section and no name for the walk | here, **open** |

**What this is not: an eval arm.** Under the three-arm program the workflow runs in the engine on
every arm, and the arms are comparable because only the treatment varies. A run under this phase
moves the sequencer, so it is a different experiment, not a fifth column of the same one. Where it
is measured against the driven native arm is cost, tokens and wall-time under the **same** governor
program — the warm-context claim above is a number to be produced, not a property to be asserted.

Steps, each its own story:

1. **Shipped.** `StepRunner` bound to a turn: a group's steps share one session, a step in a new
   group starts from `available` and nothing else, `handoff` reads the structured `answer`. Both
   emulators.
2. **Shipped.** `workflow run --flow <FILE> [--max-attempts N]`, and the whole `FlowEvent`
   inventory — `FlowStarted`, `GroupEntered`, `LayerReady`, `StepStarted`, `StepFinished`,
   `NodeSkipped`, `GroupRepeating`, `HandoffIncomplete`, `TransitionRefused`, `GroupLeft`,
   `FlowFinished`, `FlowPaused` (`crates/harness-flow/src/event.rs`) — on `--json`, rendered on
   stderr like everything else.
3. **Shipped.** The `transition` hook point, with the same *declared, never discovered; narrowing
   only* rules.
4. **Open, optional.** The metaharness projection of `flow.*`, only when an eval asks for it.
5. **Open.** Flow resume: a walk that stopped can be re-entered from where it stopped, rather than
   `--resume` being refused because a flow has one session per section.

**Exit evidence.** The binary half is reached:
`crates/harness-cli/tests/workflow.rs:1292` walks the unedited `adp-default.projected.yaml` end to
end over both emulators, and `:502`, `:634` and `:715` drive a `transition` hook that refuses a
leave, refuses an enter, and fails closed when it cannot answer — with no `metaharness` and no AEP
process alive. **Still open:** one embedded run under Phase 5's embedder doing the same through the
library, which is why the step runner's home (`harness-cli` today, reachable only with the binary)
is the design question `story:workflow-run-through-the-library` carries; and flow resume.

## Phase 9: attributable context and toolchain providers

**Status: implemented, released in `0.9.0` (2026-08-31); live polyglot evidence remains open.**

Design 0004 replaces the shell's flat standing instruction with typed layers carrying trust,
source, freshness class and a body-free digest manifest. Toolchains are now strict declarative
providers: built-in Rust, Go, Taskfile, npm and Yarn documents and explicitly loaded operator files
share read-only discovery, typed argv, selected context facts and generic verification roles. The
catalogue is fixed before turn one and only embedded substrate may admit process-local roots.

**Exit evidence:** unit and contract tests pin context rendering/manifests, static Taskfile and
package-script discovery, provider aggregation, typed argv, profile parsing, generated docs and the
inspection CLI. Still open: one delegated-cgroup run in a polyglot workspace, proving generic and
runner-specific calls execute and that a scoped formatter refuses every target before changing any.
