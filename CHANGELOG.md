# Changelog

All notable changes to this component are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **The approval gate now fires.** The loop asked its approver only for a tool whose spec said
  `Approval::Required`, and no tool this harness ships says so — so `DenyAll`, which AGENTS.md
  calls the review gate, decided nothing and `--yes` changed nothing. The loop now derives the
  question from what the **call** does: `ToolPort::invoked` answers the spec of the catalogue
  entry a `tool_invoke` names (not the verb's own, which must declare every effect any entry can
  have), and `Envelope::needs_approval` is judged against `LoopConfig::unattended_ceiling`,
  default `Risk::Low`. The same spec is what the approver is handed, what the `ApprovalRequired`
  event names and what the refusal says — `file_write`, never `tool_invoke` — and the refusal
  names the verb too, so the model keeps using it for the reads behind it; `DenyAll` says that a
  retry cannot help, and the standing instruction says not to. Consequence for a person: a
  `b10x-harness run` with a write-capable or exec-capable catalogue and no `--yes` now **refuses
  every write and every `run`** and tells the model so; `--yes` approves them. A `file_edit`
  (non-idempotent) asks whatever the ceiling. Bridge mode is unchanged: the client is the gate
  there.
- **`--substrate-embedded` is a flag, not an option.** It demanded a value it then ignored; the
  README showed it bare and no test exercised it. It is now `bool` on `run` and `tools`, and an
  end-to-end test drives the embedded path.
- **Confinement the operator named and the machine cannot provide refuses the run by name.**
  `--substrate-embedded` over a directory not named `ws_…`, an embedded driver that does not open,
  or `--substrate <socket>` with no usable daemon behind it used to fall back to the read-only
  catalogue **silently** — the operator asked for write+exec, got a read-only run, and the model
  reported the task done. Each case now exits 1 with the reason. The embedded driver is opened
  once per run instead of twice.
- **The socket client's exec asks for confinement.** `Client::exec` posted `{workspace_id, argv}`
  and nothing else — no `sandbox`, no limits — so whether it ran unconfined was the daemon's
  choice. It now probes `/v1/machine`, refuses by name when the daemon states no capability
  snapshot, and posts a body serialised from `substrate-wire`'s own `ExecStartInput` (`require:
  true`, `network: "none"`, the same limits the embedded path uses), built by one shared function
  so the two paths cannot drift. The parked socket path is still parked; what changed is that it
  can no longer run unconfined the day it is revived. Every mutating body — `exec`,
  `workspace_create`, `file_write` — now carries `op` beside `input`, the shape substrate's
  decoder requires; each lacked it and was refused `request.schema-invalid` before being read.
  The capability snapshot is asked for **once per client** and held, not before every exec, and
  the CLI probes and serves with one client, so publication and admission read one document.
- **The wall-clock deadline is checked between the tool calls of one turn**, not only between
  turns, so a turn of several slow calls stops at the first one past the deadline instead of
  running all of them. A call already running still runs to its own timeout (600 s unconfined,
  900 s confined): nothing yet passes the remaining budget into a call. It also has tests.
- **A scoped run's paths are relative, and a write is judged by where it lands.**
  `Scope::refusal` normalises `./`, `.` and `..` lexically before matching and refuses an
  absolute path when any rule is declared — a denied `target/**` used to be bypassed by
  `./target/x`, `crates/../target/x` or an absolute spelling. A rule's own glob is normalised the
  same way: `./target/**=denied` used to match nothing, silently, and an absolute or climbing
  rule is now refused when it is read. The catalogue also asks the provider where the path
  **lands** (`Operations::lands`, which `LocalOperations` answers by resolving links) and puts that
  spelling through the scope too, so a link inside the workspace (`ok/link -> target/x`) or a
  path that leaves and re-enters it (`../<workspace>/target/y`) no longer steps past a `denied`
  rule. `**/` now matches zero directories too, so `**/*.md` covers `README.md` — which also means
  `docs/**/generated.md=denied` now names `docs/generated.md`.
- `x-client-request-id` is per request; `session-id` and `prompt_cache_key` stay per run. Retry
  back-off sleeps in slices and stops on Ctrl-C. `serde_yaml` (deprecated) → `serde_yaml_ng` in
  `harness-flow`'s tests; `chacha20 0.10.1` (yanked) → `0.10.2`.

- **substrate is pinned by git revision, not reached by path.** `harness-substrate` depended on
  `../../../substrate/crates/*` — a sibling checkout, so the gate was green against whatever tree
  happened to be there and `--locked` could lock none of it. It now names `beyond10x/substrate` at
  revision `f1cfc1c` (`0.2.0` plus the brand sweep; the tag itself still carries the former brand
  in a wire hash domain). Fetching goes through the system `git` (`.cargo/config.toml`,
  `net.git-fetch-with-cli`) because the repository is private. AGENTS.md invariant 2 now says what
  the code does: no dependency on anything that could embed this, one pinned dependency below it.

### Added

- **A CI gate**, `.github/workflows/gate.yml`: `scripts/gate.sh` on `stable`, and a build on the
  declared `rust-version`. It needs the `B10X_BOT_APP_ID` and `B10X_BOT_PRIVATE_KEY` repository
  secrets to read the private substrate dependency, provisioned by atlas's `bot-ci-secrets.sh`.

### Fixed

- **`file_write` could escape the workspace through a dangling symlink.** `LocalOperations`
  tested presence with `exists()`, which follows links, so a link inside the workspace whose
  target did not exist yet looked absent; the write then followed the link and created the file
  outside. Reproduced, and reachable through `LocalOperations::unconfined`, which metaharness's
  MCP server uses. Presence is now `symlink_metadata`, a link that leads nowhere is refused, and a
  target that is itself a link is refused. Unconfined `run` no longer inherits this process's
  environment — only `PATH`, `HOME`, `LANG`, `LC_ALL`, `TERM`, `TMPDIR` and the toolchain paths
  (`CARGO_HOME`, `RUSTUP_HOME`, `RUSTUP_TOOLCHAIN`, `CARGO_TARGET_DIR`, `SSL_CERT_FILE`,
  `SSL_CERT_DIR`) reach the child, so a credential held for the harness cannot reach a program
  the model chose the arguments for; `LocalOperations::inheriting` names more, by name. A value
  that is not UTF-8 is passed as it is rather than dropped.
- **`ConfinedOperations::run` refuses an empty argv by name** rather than panicking on `argv[0]`;
  it is a public trait method and an embedder can reach it without the catalogue's check.
- **The loop's deadline tests use a 200 ms budget and 300 ms calls**, not 40 and 60, so one
  scheduling stall on a shared CI runner cannot fail them.
- `file_read` reads at most `max_bytes` from disk rather than the whole file; a truncation lands
  on a character boundary; `search` says `line_truncated: true` when it cut a matched line;
  `dir_list` reports a symlink as `symlink`. A non-string `argv` item is refused rather than
  dropped (`["cargo", 5, "test"]` no longer runs `cargo test`). Two workspaces opened with one
  lease in one process no longer share an id. The contract checkers report a corrupted fixture by
  name instead of a traceback — including a trace entry whose `frame`, or a stream event, is valid
  JSON but not an object.
- **Documentation that had drifted from the tree.** `README.md` named a `scripts/check-brand.sh`
  that moved to atlas; `STATUS.md` was dated 2026-08-21, counted 189 tests (324 pass, 1 ignored),
  omitted the `2026-08-22` wire pin and named the profile directory wrongly; design 0001 said
  nothing in it was implemented after most of it shipped in 0.1.0. AGENTS.md now records that
  bridge mode's approver is the client's, not `DenyAll`, and why.

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
