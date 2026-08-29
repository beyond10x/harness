# Changelog

All notable changes to this component are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A second wire: `anthropic-messages`, over `POST {base}/messages`.** Streaming SSE, request
  projection, tool-call decode, usage, stop reasons, cancellation and typed status mapping — the
  same loop, unchanged, behind a second projection. `b10x-harness run --wire anthropic-messages`
  selects it, defaulting to the wire this harness shipped with so every existing invocation still
  means what it did. The wire is a branch in exactly one function; below it the loop holds a
  `ModelPort` and cannot tell which projection it got.

  What the projection actually had to do, none of which the first wire needed: group a flat item
  list into **role-alternating messages** with content blocks, put a tool result in the *user*
  message that answers a `tool_use` block in an assistant one, carry tool arguments as a JSON
  **object** rather than as encoded text, send `effort` under `output_config` rather than under
  `reasoning`, and supply `max_tokens` — which this route **requires**, so absence cannot be
  preserved and resolves to a number the endpoint declares.

- **`thinking` and `redacted_thinking` blocks are opaque items.** Assembled from their
  `thinking_delta` and `signature_delta` fragments, kept whole, and replayed byte for byte **and in
  place** — nothing reorders content blocks, which is what keeps a thinking block first in its
  message without this code having to know why that matters. The reasoning text is never emitted to
  a reader: opaque means opaque. Replaying one into the Responses wire, or a `reasoning` item into
  this one, is a typed refusal naming both wires rather than a silent drop (invariant 5); both
  directions are now tested at the client, not only at the type.

- **`contracts/provider-wires/anthropic-messages/2026-08-29`**, checked from both directions like
  every other pin. It adds two halves the first wire has no equivalent of: the
  `content_block_delta` sub-types — on this route the interesting variation is *inside* one outer
  event name, so pinning the outer names alone would pin almost nothing — and the **header names
  each credential kind travels under**, checked against the same function the client calls to build
  them.

- **`harness-credential`, and a `BearerSource` for a subscription token.**
  `SubscriptionToken` reads a token from a file or an environment variable the caller **names**,
  optionally at a caller-named JSON pointer, and re-reads it on **every** call — so a token an owner
  outside this process renews is followed without restarting the run. There is no default path, no
  vendor directory it looks in, and no fallback when the named source is missing: a source that
  searched on failure would be an ambient credential fallback whichever way it was spelled. New
  flags: `--oauth-token-file`, `--oauth-token-env`, `--oauth-token-pointer`, mutually exclusive with
  the API-key flags.

  Its own crate rather than part of a wire, because **nothing about it is vendor-shaped**: the two
  subscription routes this harness cares about hang off two different wires, and putting the source
  in one would make the other depend on it to reuse it. What *is* vendor-shaped is how the fetched
  credential is presented, and that stays in the wire crate.

- **Both wires pass the same loop suite.** `harness-messages`'s provider-emulated suite is
  `harness-responses`'s, case for case, over a second deterministic local endpoint with the same
  scenario names — and `the_two_wires_serve_the_same_scenarios` compares the two emulators' own
  declarations, so a case added to one and not the other fails the gate instead of being noticed a
  release later.

### Changed

- **`harness_wire::Usage` gained `cache_creation_input_tokens`,** an `Option`. The second route
  bills cache *writes* as their own class; dropping the figure would make a cache-writing turn
  indistinguishable from one that wrote nothing. It is optional because a route that never mentions
  cache writes has **not** said there were none (invariant 7). Nothing prices it separately — the
  rate card has no cache-write field — so it is counted inside `input_tokens` and priced at the
  input rate, which understates such a turn; carrying the figure is what makes that visible.

  `Usage` now also documents the invariant it had only ever implied: **`input_tokens` is the whole
  and the cache figures are parts of it.** The second route reports its three input figures
  *disjointly*, so its projection sums them — left unsummed, every cached turn would have reported
  fewer input tokens than it was charged for and priced itself low.

- **`harness_wire::BearerSource` gained `kind`,** defaulted to `CredentialKind::ApiKey`. One
  endpoint, two routes, the same secret under **different header names** — so which kind a
  credential is stopped being derivable from the wire alone and became a property of the source.
  The kind is neutral; the header names (`x-api-key`, `authorization`, `anthropic-beta`) stay in the
  wire crate, which is where every vendor-shaped byte belongs (invariant 3).

### Known gaps

- **The transport half of the two wires is duplicated, and that is this change's real finding.**
  Bounded SSE framing, the retry rule, the witnessed sink that makes the retry rule safe, the
  back-off and the status mapping were copied from `harness-responses` unchanged, because none of it
  is vendor-shaped — it is *transport*-shaped, and the first wire could not tell the difference
  while it was the only one. A `harness-http` beneath both is what that argues for. It was
  deliberately not done here so that this change is the evidence rather than a guess acting on
  itself.
- **No subscription route has been contacted, and nothing renews a token.** The Anthropic header
  shapes are `provider_emulated`; the emulator records which header carried a credential and its
  length, never its value. A token nobody renews expires and the run fails by name.
- **The conversation is not prompt-cached on the second wire.** One `cache_control` breakpoint
  covers the constant head (`tools`, then `system`); the growing tail — which is what makes a
  stateless run's cost quadratic in its turns — needs a placement rule there is no measurement for
  yet.

## [0.1.0] — 2026-08-24

First tagged release. The entries below cover everything since the component was established;
the commit history carries the full reasoning per change.

### Fixed

- **Compaction reaches its target instead of firing every turn.** The floor on what a compaction
  may elide was a count — the newest six tool results were never touched — and six results can
  outweigh the whole target, so compaction fired on consecutive turns and each rewrite voided the
  prompt cache for a full-rate replay. The floor is now bytes (`KEPT_RESULT_BYTES`, 48 kB) and
  compaction elides to a low-water mark (`COMPACTED_TARGET_BYTES`, 96 kB) instead of stopping the
  moment it fits. Measured on a live run: one compaction instead of four, cost −17%, cache hit rate
  78% → 86%.
- **A confined read is bounded, and says when it was.** The substrate-backed `file_read` ignored
  `max_bytes` and always answered `truncated: false`; the note claiming the truncation could not be
  reported was wrong. It now bounds at 64 kB — the same figure the unconfined provider uses — and
  reports the real size and `truncated: true`.
- **A turn the far side never answered is retried**, instead of ending the run before any text
  arrived.
- **The `2026-08-22` provider-wire manifest is re-pinned to its own fixture.** The workspace tool
  rename changed `turn-stream.sse` without moving the manifest digest, so the contract check
  refused bytes the Rust contract test already required.
- **A tool name this wire cannot publish is refused before the request, and the workspace toolset is
  renamed.** The first live run this component has ever had — `https://chatgpt.com/backend-api/codex`
  under a ChatGPT subscription credential, 2026-08-23 — answered turn 1 with

  ```text
  400 Invalid 'tools[0].name': string does not match pattern.
      Expected a string that matches the pattern '^[a-zA-Z0-9_-]+$'.
  ```

  The published toolset was `workspace.list` / `workspace.read` / `workspace.grep`, and had been
  since the crate was written. Nothing caught it because the only endpoint that had ever seen a
  request was the emulated one, and an emulator written from the same source as the projection
  cannot disagree with it about what a provider will take. This is the class of defect
  `STATUS.md` predicted with *"all evidence is `provider_emulated`; it proves nothing about how a
  real provider behaves"* — the prediction was right on the first attempt.

  The tools are now `workspace_list`, `workspace_read` and `workspace_grep`, and
  `harness-responses` gained `check_tool_names`, called beside `validate` and `check_opaque_items`
  in `turn`. It refuses a toolset this wire cannot carry **locally**, naming the offending tool, the
  pattern, and the name that would work.

  **The rule is in the wire, not in `harness-wire`.** `ToolName` still admits any printable ASCII
  identifier, and a test pins that it admits a dot. The pattern is one provider's, verified against
  one provider; putting it in the neutral crate would shape the neutral layer to a single vendor and
  forbid a name the Messages wire may well accept. A dedicated test in `harness-wire` exists to stop
  a later reader tidying it back in.

### Added

- **A workflow notation the loop runs natively** (`harness-flow`): a DAG of sub-trees, a group as
  a context scope with what crosses it written down, a retreat as a group that repeats — because a
  DAG has no back-edge — and plan/walk over a real projected workflow, with the verdict split from
  the tallies. A workflow renders as committed prose instructions.
- **Confined tools, published only where the machine can confine them.** `file_write`, `file_edit`
  and `run` exist behind substrate's own contract: what this machine can confine is read from
  substrate's facts, an embedded driver rides behind the same trait as the socket, and publication
  follows — three tools with no backend, five with an embedded driver, six inside a delegated
  cgroup. `--substrate-embedded` and `--cgroup-root` on `run` and `tools`; one tree, so the
  workspace a run reads is the workspace it writes.
- **A declared toolchain** (`--toolchain rust`), so a confined run can build and not only
  interpret: exec limits sized for a build, the exec identity substrate admits, and a pin that the
  declared toolchain carries no operator credential into the child.
- **Three verbs over one catalogue**: `tool_search`, `tool_describe` and `tool_invoke` over
  entries named by neutral operations; a call names which file it touched, and the run's own
  record — the event stream every arm is judged from — reports what it cost.
- **A run declares where it may write, and the toolset holds it**: `--write-scope
  <glob>=<allowed|partial-only|denied>` (ordered, first match wins, unnamed paths unrestricted),
  `--context <file>` preloaded into the standing instruction (an absent file refuses the run), and
  `--scope-announce stated|silent` — `silent` is the experiment control that shows the toolset,
  not the prose, is what holds the rule.
- **Prompt caching on the Responses wire**: send `prompt_cache_key`, key it on the conversation,
  say who is calling, and carry the standing instruction at the head of `input` where the cache
  can see it; the catalogue is stated once in the instructions instead of asked for, call by call.
- **A conversation bound instead of a length cliff**: the loop elides old tool-result payloads
  when the replayed conversation passes its bound, and the warning carries the figures.
- Carry sampling on a turn. `TurnRequest` gains an optional `Sampling` — temperature, top_p and a
  reasoning effort — which `LoopConfig` sets once and the loop sends on every turn, because a
  stateless loop replays the whole conversation and a value carried only on the first request would
  apply only to the first request. `b10x-harness run` exposes `--temperature`, `--top-p` and
  `--reasoning-effort`.

  A field nobody set is **absent**, not defaulted. Writing a provider's own default here would take
  a decision that provider is entitled to make and change, make it ours, and make it invisible: a
  request carrying `temperature: 1.0` looks identical to one somebody chose. Values outside their
  range are refused before the request is sent, because the round trip otherwise costs a turn and
  returns a vendor error string nobody can act on.

  Because the request field set changed and a pinned wire version is immutable, this opens
  `contracts/provider-wires/openai-responses/2026-08-22/`. The response side did not move, so the
  stream fixture is byte-identical to the previous version's. `effort` is nested under `reasoning`;
  a flat `reasoning_effort` is accepted by the transport and ignored by the provider, and a test
  pins the nesting for that reason.

  This contract says the fields are **sent**, not that any endpoint acts on them. The self-hosted
  gateway fixes thinking and effort when it launches a pod, so a per-request effort reaches it and
  changes nothing. Which endpoint honours what is `runtime/agent`'s route registry's question.

- Establish `runtime/harness`, B10x's own agent loop, as a component separate from the Codex
  and Claude bridges in `runtime/agent`. It carries no bridge and depends on nothing else in the
  monorepo; the arrow points inward, so a future consumer embeds it rather than the reverse. The
  split is accepted by architecture ADR 0052.
- Add `harness-wire`: neutral conversation items, tool specifications, turn requests and outcomes,
  reported usage, stream events, size bounds, and the three ports the rest of the component is
  built on — `ModelPort`, `ToolPort` and `BearerSource`. It performs no I/O, reads no clock, holds
  no credential and names no vendor field, which is what lets a second wire cost a projection
  rather than a second loop. A provider item the component does not model is carried as an opaque
  value tagged with the wire that produced it; replaying one into a different wire is a typed
  refusal rather than a silent drop, and carrying reasoning items verbatim is what keeps a
  stateless loop as capable as a provider-threaded one.
- Add `harness-responses`: the `openai-responses` wire over `POST {base}/responses` in streaming
  mode. Bounded SSE reading refuses an oversized event, an oversized stream, an unparseable
  payload, and a stream that ends mid-event — a truncation is never read as a completion. Requests
  are stateless (`store: false`, the whole conversation replayed) and ask for encrypted reasoning
  content. HTTP statuses map to actionable codes: a rejected key is a non-retriable
  `Unauthorized`, a starting gateway is a retriable `Transport`. Arguments that are not JSON never
  reach a tool. Unreported usage stays absent rather than becoming zero.
- Add `harness-loop`: turn assembly, tool round trips, approvals and budgets. Because the loop is
  owned here it can count `max_turns`, input and output token totals, and a wall-clock deadline,
  so those bounds are enforced rather than hoped for; a spend ceiling is refused by name before the
  first request, since a gateway relays bytes and reports no price. An approval is an ordinary
  blocking call, so a decision cannot arrive after the effect, and the default approver denies. A
  call the run never published, a denied approval, an oversized argument set and an oversized
  result all return to the model as failed outcomes, so it learns the effect did not happen; the
  oversized payload is kept out of the replayed conversation so one bad call cannot poison every
  later turn.
- Add `b10x-harness`, the command-line shell, with a bounded read-only workspace toolset —
  `workspace.list`, `workspace.read`, `workspace.grep` — that refuses any path resolving outside the
  workspace, including through a symlink, and reports its own truncation rather than implying a
  partial answer is whole. Credentials come from an explicitly named file or environment variable
  with no ambient fallback. Ctrl-C ends the run rather than the process, cancelling both the loop
  and the response body being read. Exit status distinguishes an answer, a named stop, and a
  harness that could not run.
- Pin the wire in `contracts/provider-wires/openai-responses/2026-08-21`: the exact request the
  harness sends and the exact stream it accepts. Both halves are checked —
  `scripts/check-provider-wires.py` verifies the manifest against its fixtures, and a Rust contract
  test verifies the harness actually produces those bytes.
- Prove the composition against a real socket. A standard-library local Responses endpoint drives
  fifteen provider-emulated cases through the real client and the real loop, and seven end-to-end
  cases through the built binary over a real workspace. This is `provider_emulated` evidence and is
  never promoted to a claim about a real provider; no live run has happened.
- Register the component in the monorepo gate, `scripts/check-local.sh`.
- Add bridge mode: `b10x-harness app-server` serves B10x's own loop over the pinned
  Codex app-server JSON-RPC format on stdio, under the client's operation-tools profile. The real
  bridge has not driven it and no gate compares the two inventories; all evidence is this
  component's own client. `runtime/agent` already drives a process speaking that
  format and the command it spawns is arbitrary, so this reuses that entire bridge — its conformance
  suite, its governed execution lane, its process reaping — with no new bridge code and no
  dependency in either direction. A protocol is the seam; a shared crate would have been a coupling.
  Tools arrive from the client as `dynamicTools` on `thread/start` and are called back over the
  wire, which makes the bridged tool port the second implementation of the same `ToolPort` the
  embedded shell uses: in-process a tool call is a function call, here a round trip, and the loop
  cannot tell. `thread/resume` and `turn/steer` are refused by name rather than answered with a
  silent success, because a client told a thread resumed or a turn was steered would carry on
  believing something happened that did not. A run stopped by a budget is reported `failed`, not
  `completed`, and a failed or interrupted turn delivers no answer alongside its terminal frame.
- Make cancellation reach the layer that is actually blocked. One shared token now spans the loop,
  the tool sequence and the HTTP response body being read, replacing the per-layer flags; in bridge
  mode the reading thread sets it the instant a `turn/interrupt` frame is decoded, and the server
  acknowledges it between streamed events. A turn spends almost all its time blocked on the model,
  so an acknowledgement that waited for the main thread to return to the wire would arrive after the
  turn it was meant to stop had already finished.
- Treat a cancelled model read as a terminal outcome rather than an error, in every shell. A person
  who presses Ctrl-C was previously told the model wire had refused; the run now ends as cancelled,
  keeping the work it did complete and the usage it reported.
- Pin the served JSON-RPC subset in `contracts/app-server-profile/codex-app-server-stdio-v2/`, with
  a complete connection trace and a manifest. `scripts/check-app-server-profile.py` proves every
  frame is a declared method, every request is answered and every declared method is exercised; a
  Rust contract test proves the server's own constants match the manifest. The method inventory is a
  deliberate copy of the client's rather than an import — copying is what keeps the components
  independent, and nothing here can check it against the original, so a Codex version bump is a
  review obligation rather than a gated one.

### Fixed

Found by an independent review of the two slices above, before any of it was released.

- Stop bridge mode aborting the process whenever a client sent anything while a turn was running.
  Writing a notification held a borrow of the connection that draining control frames then took
  again, so a pipelined `turn/start` + `turn/interrupt` — the exact sequence an interrupt is for —
  killed the server with no terminal frame at all. A bridge saw only a dead pipe. Covered now by
  two regression tests that reach the interleaving through different frames.
- Declare the profile the client actually offers for tool calling. Bridge mode announced
  `codex-app-server-stdio-v2`, which is the client's *stable* profile: it registers no dynamic
  tools, refuses `item/tool/call` as an out-of-profile method, and cannot classify a
  `dynamicToolCall` item. The server would have looked compatible and failed at the first tool
  call. It now declares the operation-tools profile, requires the client to negotiate
  `experimentalApi` before accepting a tool registration, and refuses that registration by name
  otherwise rather than stranding the turn later.
- Give each turn its own cancellation token. A single token cleared at the start of every turn
  raced the reading thread: an interrupt decoded just before the clear was erased, and the turn it
  was meant to stop ran to completion while the client held an acknowledgement. `Cancel::reset` is
  removed, so the shape cannot come back.
- Report an interrupt that was actually requested as `interrupted` even when the connection drops
  afterwards. A later write failure used to overwrite it, reporting a person's own cancellation as
  a fault.
- Stop `workspace.grep` reading outside the workspace. The supplied path was checked, but the walk
  then followed a symlink inside the workspace and returned outside files under a workspace-relative
  name. Every entry is now re-checked after canonicalization. The previous symlink test covered
  `read` only.
- Preserve a failed tool result at the Responses wire. The projection dropped the `failed` flag, so
  a bridged client answering `{"success": false, "contentItems": []}` reached the model as an empty
  *successful* call — the exact failure mode the loop's refusal design exists to prevent.
- Cap both framers at the read rather than after it. A peer that never sent a newline chose how much
  memory this process allocated: measured 606 MiB before the 32 MiB stream bound was consulted.
- Bound the wait for a tool answer. A client that never answered `item/tool/call` held a turn open
  forever.
- End the turn when the client is gone. A broken pipe stopped the writing but let the loop keep
  calling the model, spending inference for a reader that would never see it.
- Answer every tool call the model made, even ones a cancellation skipped. A `function_call` left
  without its output makes the conversation unreplayable, so a cancelled run could not be resumed.
- Align the two bridge bounds with the client's: tool answers 64 KiB → 256 KiB, frames 4 MiB →
  8 MiB. The smaller values refused traffic the client is entitled to send, and the tool bound's own
  doc comment claimed it already matched.
- Use saturating arithmetic for the two token sums that were not, which panicked in debug and
  reported `totalTokens: 0` in release on a hostile usage report.
- Require the profile contract to exercise every declared *client* method, not only server ones.
  `turn/interrupt` was declared and untraced, which is how a crash on the interrupt path reached a
  green gate.
- Correct the documentation the review falsified: the `harness-wire` test count (33 → 26, the
  headline was right and the table was not), "the gate keeps the copy honest" (nothing compares the
  copy to the original), "`harness-wire` holds no credential" (it defines `StaticBearer`, which
  holds one for as long as its caller does), and the provider-wire README crediting the Python
  checker with checks the Rust test performs and with fields nothing checks at all.
