# Changelog

All notable changes to this component are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Structured output, sub-agents and hooks** — the three of finding #13's five gaps that are this
  component's to own (`docs/design/0002-sub-agents-structured-output-hooks.md`; the MCP client and
  multimodal input stay out, with the reason in `README.md`). All three are opt-in per run, none
  touches `harness-wire`, and every one of them meets the approval gate exactly as a catalogue entry
  does: nothing reaches a tool without the gate, nothing widens what a turn admits, nothing refuses
  silently.
  - **`--output-schema <FILE>`.** The schema is published as a tool named `answer` that the model
    calls to finish, and its arguments are the answer — wire-neutral, no contract change, and what a
    delegate's structured report will be built on; provider-native constrained decoding behind the
    same value is a labelled later milestone. **Stdout is that JSON and nothing else**, so the
    command composes with `jq`; it is written once, when the run completes, so an answer a `stop`
    hook withdrew never reaches it. Under `--json` stdout is the event record instead, with no
    bare answer line: the answer is the **last** `answered` event before a `finished` whose
    `stop.kind` is `completed` — a `stop` hook can withdraw an earlier one, so a driver taking the
    first takes a refused value. A model that ends in prose is told once to call `answer`; if
    it still does not, the run stops `unstructured` and exits 2 — never a success status over
    prose. An `answer` beside any other call in one turn ends the run and refuses the others as
    *made in the same turn as `answer`, which must be called alone* — which is what the tool's
    description promised, and a sentence that stays true when the answer itself is refused and the
    run goes on. A `stop` hook that sends an answered run back to work restores its nudge, so a
    second prose ending is asked once more instead of exiting `unstructured`.
    The nudge is warned as `answer-nudged`; a `{"accepted": true}` result goes into the
    conversation so the run stays replayable; `LoopEvent::Answered` puts the value in the record;
    the loop validates nothing against the schema. The session stores the answer beside the text.
    `chat` does not take it — a conversation has no single end.
  - **`--delegate`** (`--delegate-turns N`, default 20). A tool named `delegate`: a second
    `AgentLoop` runs to completion inside the tool call over a **fresh** conversation, with the same
    tools, the same approver, the same hooks, the same cancellation token and the **remainder of
    the parent's budget** — a delegate spends the run's budget, never its own, and the parent's
    ceilings bind on the sum — on every exit path, a child that failed on the wire included. The
    parent reads one result, `{stop, turns, text}`, failed when the child did not complete; every event the child emits arrives wrapped in `delegated` so a reader
    cannot mistake its text for the answer, and a terminal renders them indented. Depth one: a
    delegate cannot delegate, and it publishes no `answer` either. `--delegate-turns 0` is a parse
    error (exit 2), refused where the parent's own `--max-turns 0` is refused — before the first
    request — rather than as a failed tool result on every delegation. A port that already publishes
    `answer` or `delegate` refuses the run by name (`LoopError::Config`) **before the first
    request**, rather than being found out by a wire rejecting a duplicate tool on turn one.
  - **`--hooks <FILE>`.** The operator's own programs, run as an argv — never a shell — at three
    moments: `before-call`, after the approver said yes, where exit 2 refuses the call and a hook
    that could not run refuses it too; `after-call`, where a note is appended to the result the
    model reads; `stop`, where exit 2 keeps the run working with the reason as the next user item,
    at most three times — and never at the end of a delegate, whose ending is not the run's. A
    hook can refuse what the gate allowed and can allow nothing the gate refused; `answer` and
    `delegate` are calls like any other to a hook. Named on the command line and **never discovered in the workspace**: a hook found in a
    repository would be a program the repository runs on the operator's machine. A run with hooks
    attached batches nothing, so a hook fires exactly once per call, and every firing is a
    `hook-ran` event naming the point and the decision. The refusal names the entry that would
    have run — *"`run` (called through `tool_invoke`) was blocked by a hook: …"* — the same way an
    approval refusal does. An `after-call` hook's exit 2 or failure becomes a note rather than
    silence — and a failure is recorded as `hook-ran` with `decision: failed`, never `proceed`, so
    the record shows a guard that crashed (`HookPort::after_call` returns `AfterCall { note,
    decision }`). `after-call` does not fire for a call that never ran — an unpublished tool, an
    argument over the bound, a call the approver refused — those are in `tool-completed` and
    `approval-resolved`. A note that pushes a result over the result bound refuses the result by
    name. On
    the command line: exit `0` proceeds, `2` blocks with the reason from `{"reason"}` on stdout or
    else stderr, any other status, a program that cannot start, more than 16 KiB on stdout or 60 s
    of running — pipes included, so a grandchild holding them cannot stall the run — fails by
    name; the child never inherits the variable this run's own credential was named in; a `stop` hook declaring `tools` is refused at load, because nothing
    would ever match it. A hooks file this build cannot read, and a schema that is not an object
    schema, refuse the run before the first request like every other run that never started. The
    argv pin `contracts/cli/b10x-harness/2026-08-29` is re-pinned in place — it is unreleased, and
    invariant 13's immutability starts at release — and now records `requires` per flag beside
    `conflicts_with`, so a consumer can see that `--delegate-turns` needs `--delegate`.

- **The model is handed the tools themselves.** `--surface flat` — the **default** on `run`,
  `chat` and `tools` — publishes every catalogue entry as its own tool with its own input schema,
  so the provider can refuse a misspelled field before the call is billed and no turn is spent
  finding out what exists. Three live runs measured the cost of the alternative: **33–44% of every
  tool call was `tool_search` or `tool_describe`**, and `tool_invoke.arguments` was an untyped
  object nothing could validate. The neutral names the three verbs existed to protect are the
  entry names themselves (`file_read`, `file_write`, …), which `harness_tools::operation_of` maps
  for a reader of a finished run, so nothing downstream loses vocabulary. `--surface verbs` is
  unchanged and fully served: metaharness offers it over MCP, and an arm comparing the two
  surfaces asks for it by name. The standing instruction follows the surface — under `flat` it
  names the entries in one line and leaves the schemas in `tools`, where the provider reads them
  and a prompt cache holds them.

- **Sessions on disk, and `--resume`.** A run that dies on turn 20 no longer takes the first
  nineteen with it. `AgentLoop::run_in(&mut items, &mut spend, input, sink)` runs over a
  conversation and a `RunLedger` the caller owns and writes both back on **every** exit path,
  including the two that are errors — `LoopError` carries neither items nor usage, which is why
  nothing could be saved before. The command line files
  it: `transcript::Session` writes the whole conversation, its usage and its cost to
  `$XDG_STATE_HOME/b10x-harness/sessions/<id>.json`, atomically, in a directory created `0700`,
  outside the repository. Items are stored verbatim, opaque reasoning items included, so a
  following run replays what the model already thought instead of paying for it again. No
  credential is written, and no instruction text: the instruction is derived from this run's
  catalogue and files, and replaying under a stale one would give a run nobody could reproduce
  from its flags. New flags: `--session-dir <path>`, `--resume <id|latest>`, `--no-session` (for
  an evaluation arm that must leave nothing on the machine). A session recorded on the other wire
  is refused **before the first request**, by name, with the flag that fixes it — the loop would
  refuse the opaque items anyway, and saying it here costs nothing; a different workspace is a
  warning, because reading a second checkout is a legitimate thing to do.

- **`b10x-harness sessions`** lists what there is to resume — identifier, UTC timestamp, model,
  turns — newest first.

- **`b10x-harness chat`**, the smallest thing that removes *one question, one answer, exit*. Every
  line of standard input is one more turn on the same conversation, the session is written after
  each of them, and `exit` or the end of the input stops. The same flags as `run` without
  `--input`. No line editing, no history, no completion: a shell has all three, and a harness that
  grew them would own a terminal library forever.

- **A person can approve one write and refuse the next.** `--approve <auto|prompt|deny|all>`,
  default `auto`. `approve::Terminal` asks over `/dev/tty`, so the question arrives even when
  stdin and stdout are pipes, and the prompt names the entry the call resolved to — `file_write`
  with its path and byte count, `file_edit` with the first lines of both sides, `run` with its
  argv — never the verb it travelled through. `y` approves once, `a` stops asking about that entry
  for this process only, `n` and an empty line refuse; nothing answering refuses every further
  call, said once. `auto` asks when there is a terminal and stdin and stderr are one, and
  otherwise prints a single line saying calls above the ceiling will be refused rather than
  leaving it to be discovered from a refusal. `prompt` refuses the run when there is no terminal —
  a run that asked for a person and silently refused everything looks like a harness whose tools
  do not work. `--yes` is unchanged and is the same as `--approve all`. **The library's default
  approver is still `DenyAll`** (invariant 12); what changed is the command line's choice.

- **The model is told where it is.** With no `--instructions-file`, the standing instruction now
  carries an environment block — the absolute workspace path, the OS and architecture, today's UTC
  date, and the git branch, read from `.git/HEAD` and following a `.git` file to a linked
  worktree, **never by spawning `git`** — and the project's own instruction file, `AGENTS.md`
  before `CLAUDE.md` because the neutral one is the maintained one. Anything past 32 KiB is cut at
  a line boundary and the instruction says in words which part of how many bytes was carried.
  `--no-project-instructions` leaves the project's words out as an experiment control; the
  environment block is always there.

- **`find`, a seventh catalogue entry.** Name a glob and get every matching file in one call,
  instead of one `dir_list` per directory level: `*.rs` is that file name at any depth,
  `crates/**/*.rs` is the whole workspace-relative path. The same walk as `search` — build output
  and version control skipped, depth 12, containment re-checked per entry — capped at 500 paths
  with `truncated` when it binds.

- **`search` takes `regex`, `glob` and `context`.** A regular expression that does not compile is
  refused in the regex crate's own words rather than quietly matching nothing; `context` (0–5)
  answers the lines either side of each match under `before`/`after`, each with its own number.

- **Pure tool calls of one turn run side by side.** A turn that asks for six independent reads no
  longer pays six round trips of tool latency: consecutive calls that are published, inside every
  bound, and whose invoked envelope neither mutates nor asks a person are handed to the port as
  one batch (`ToolPort::call_batch`, one thread per call in `Catalogue::invoke_batch`). A write
  between two reads ends the group; a group of one goes down the single-call path unchanged. A
  port that answers a different number of outcomes than it was given calls is **not trusted with
  any of them** — the loop says so by name (`batch-miscounted`) and runs every call itself.

- **A long think is no longer a silent minute.** `response.reasoning_summary_text.delta` on the
  Responses wire and `thinking_delta` on the Messages wire become `StreamEvent::ReasoningDelta` and
  reach a reader on stderr as they arrive. Shown and let go: nothing here is replayed, and what
  carries reasoning across a tool round trip is still the opaque item the turn ends with. The
  Responses summary's `.done` and `part` markers stay silent because each repeats text already
  streamed, and a thinking block's signature is never shown at all.

- **`contracts/provider-wires/anthropic-messages/2026-08-29b`: the Anthropic conversation is
  cached, not just its head.** A run's transcript is resent whole every turn, so with one cache
  breakpoint on the constant head every byte the conversation grew by was paid at full rate on
  every remaining turn — a measured 81-turn run watched its hit rate fall from 66% to 12.5% and
  spent 1.33M input tokens to produce 10.5k of output. A second, **rolling** breakpoint now marks
  the last block of the last message, so each turn writes the prefix it just read and the next
  turn reads it back. Two breakpoints against a documented cap of four, and never on a replayed
  `thinking` block: the provider's signature covers those bytes, so marking one would be a
  rejected turn (invariant 5). **`2026-08-29` stays as released and is superseded by `2026-08-29b`
  wherever this changelog names it.**

- **`contracts/cli/b10x-harness/2026-08-29`: the argv surface is a contract now.**
  `--substrate-embedded` changed from taking a value to being bare — the right change — and a
  consumer pinned to `0.1.0` went on passing a value, which clap refused before any harness code
  ran. The wire contracts pin what goes to a model and the profile contract pins what a bridge
  client sees; the command line is a **third** interface with consumers of its own. `argv.json` is
  generated from clap's own definition (every long flag, whether it takes a value, its value name,
  its default, whether it is required, and every flag it conflicts with, in both directions) and
  checked from both sides: `scripts/check-cli-contract.py` against the manifest digest, and a Rust
  test against clap, failing with a diff that says to cut a new version.

- **A run that never starts leaves a terminal record.** A driver launched this binary with a flag
  that had changed shape; clap wrote its usage and exited **2** before any harness code ran, and
  the driver — which reads the `--json` record and the exit status — saw a status it already had a
  meaning for and an empty stream. Two hours went into working that out. Now every refusal that
  happens before the loop starts — a refused command line, a credential, a workspace, a
  confinement, a session on the wrong wire — writes one line, `{"kind":"refused","reason":…}`, on
  stdout under `--json` and exits **1**; on this command line `2` means *the run stopped for a
  named reason*, which is a run that happened. `b10x-harness events` maps it onto the
  `session.ended` record the stream already has, `subtype: "refused"`, with the reason in
  `stop_reason`.

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
- **`--approve-up-to <risk>`** on `run`: raises the loop's unattended ceiling (`low` by default)
  so a `file_write` (`medium`) or a `run` (`high`) goes through without asking, while everything
  above it still asks and — with no approver attached — is refused. A `file_edit` asks whatever
  the ceiling, because it is non-idempotent, and still needs `--yes`; the two flags do not
  combine, since `--yes` approves everything.

  *Superseded 2026-08-29: idempotency no longer asks. `Envelope::needs_approval` is `risk >
  ceiling` and nothing else, and `file_edit` is `medium` like `file_write` — so
  `--approve-up-to medium` lets both through and neither needs `--yes`. See § Changed below.*
- **A CI gate**, `.github/workflows/gate.yml`: `scripts/gate.sh` on `stable`, and a build on the
  declared `rust-version`. It needs the `B10X_BOT_APP_ID` and `B10X_BOT_PRIVATE_KEY` repository
  secrets to read the private substrate dependency, provisioned by atlas's `bot-ci-secrets.sh`.

### Changed

- **`file_read` stops counting lines after 16 MiB.** Counting `lines.total` had become a full
  sequential scan of the file on every read, which a deadline cannot reach into; past the bound
  `lines.total` is `null` and `lines_counted_to` says where the scan stopped. `bytes` is still the
  file's own size.
- **A batch runs at most 8 calls at a time** instead of one OS thread per call — a turn asking for
  two hundred reads is two hundred reads, not two hundred threads.
- **`search` compiles a regular expression under a 1 MiB size and DFA limit**, refusing one over it
  in the crate's own words, and echoes `context` when it capped it at 5.
- **Both wires stop calling an error retriable once they have made four attempts**, whatever was
  emitted. A turn that failed three times cold and then broke mid-stream used to buy the loop
  another three rounds of four — sixteen requests and half a minute to learn one thing.
- **The transport half of both wires is one crate now, `crates/harness-http`** — a new name in the
  crate list, which is why an internal move gets an entry here. **No behaviour change.** Bounded
  SSE framing, the retry rule, the back-off, the witnessed sink that makes the retry rule safe, the
  status mapping and the blocking client with its two timeouts moved out of `harness-responses` and
  `harness-messages`, which had held byte-identical copies since the second wire was written; each
  wire is now its projection, its URL and its headers over `harness_http::HttpTransport`, and
  neither depends on `reqwest` at all. `harness-wire` is untouched, and no vendor name, field name
  or header name appears in the new crate.
  **What proves nothing moved:** the two pinned contract suites, `scripts/check-provider-wires.py`
  and both `provider_emulated` suites pass unchanged — not one fixture, manifest or case was
  edited. One real difference between the two copies was found and is now explicit rather than
  implicit: the first route ends its stream with `data: [DONE]` and the second has no sentinel at
  all, so `Framing` is a per-wire setting and `crates/harness-messages/tests/transport.rs` fails if
  the wires ever disagree about anything else. The status tables were already identical — 529 was
  covered by the 5xx range on both sides, and only the comments differed.
- **Bridge mode compacts on the context window too.** `ServerConfig::context_window` carries
  `--context-window` into every bridged thread's `LoopConfig`, the same as `run` and `chat`.

- **Idempotency no longer asks for approval; risk alone does.** `--approve-up-to high` let a `run`
  and a whole-file `file_write` through unasked and refused every `file_edit`, because a second
  clause asked about every non-idempotent mutation whatever the ceiling — a retry question written
  into an approval gate. An unattended run was being pushed toward rewriting files whole when the
  narrower edit was the safer act. `Envelope::needs_approval` is now `risk > ceiling`, and
  `file_edit` and `file_write` are both `Medium`. `Idempotency` is still declared, for a scheduler
  that re-runs a scope to read.

- **Any part of any file is readable, and what comes back is numbered.** `file_read` takes
  `offset` and `limit` in lines and answers numbered lines in `cat -n` shape — the numbers are
  what let a model quote exact text back to `file_edit` — plus `lines: {from, to, total}`, so a
  window is never mistaken for a whole file. A line over 2,000 characters is cut and its number
  listed in `truncated_lines`, never silently. A window that starts past the end of the file is
  refused with the number of lines there are; the confined path refuses by name saying which line
  its byte ceiling reached.

- **A test suite's verdict survives a long `run`.** Output over the 64 KiB cap kept the **first**
  64 KiB and dropped the rest — which is the compiler's progress and never `test result: FAILED`.
  Both ends are now kept with `\n… N bytes omitted here …\n` between them, and the result reports
  `omitted_bytes`.

- **`harness_tools::Operations` is a breaking change for an out-of-tree implementor.** metaharness
  embeds this crate to serve the same catalogue over MCP, so it is named here rather than left to
  be discovered at a build: `file_read(path, ReadWindow)` and `search(pattern, path,
  &SearchOptions)` take the new argument shapes, `find(...)` is a new method with a **defaulted
  refusal** so an implementor that does not answer it refuses by name instead of failing to
  compile, and the trait is now `Operations: Send + Sync` — required by `Catalogue::invoke_batch`,
  which gives each call of a batch a thread.

- **The model may call a catalogue entry by its bare name.** 10 of 82 tool calls on one live run
  were `file_read{path}` rather than `tool_invoke{name:"file_read"}`, each refused as unpublished
  and each a dead turn. Under `--surface verbs` the published list is still the three verbs; a
  bare name is routed to the entry and warned about (`unpublished-tool-routed`) so the waste stays
  measurable. Routing widens nothing: the entry was already reachable through the verb, and it
  meets the same approval gate, the same argument bound and the same result bound. The metaharness
  converter reads a routed call as the act it performed, under either surface.

- **Compaction can see the context window, and `--context-window` now drives it.** Given a window
  in tokens it fires at 80% — measured by the provider's own last reported input count, or by a
  bytes÷4 estimate where nothing reported — and frees down to 50%. Where eliding old tool output
  cannot reach the target, because the weight is in user or assistant text or in opaque reasoning
  items, the harness spends one extra turn asking the model to summarise the earlier part of the
  run and replaces it with a single marked item; the task itself, the newest results and every
  call-with-its-result survive. That turn is charged to the run's tokens, budget and bill like any
  other and does not count against `max_turns`; a summary that fails on the wire is a warning
  (`summary-failed`), not the end of the run. Without a declared window the fixed 192 KiB byte
  rule is unchanged. `b10x-harness run` and `chat` pass `--context-window` into `LoopConfig`, so
  the flag that had only ever bounded the request now also decides when the conversation is
  compacted.

- **A failure that may not repeat now says so.** A stream that stops mid-frame or closes before its
  terminal event is a dropped connection, not a peer speaking a different protocol, and is
  reported as retriable; `408` joins `429` and `5xx`; on the Responses wire a `server_error` or
  `rate_limit_exceeded` **in the stream** is the provider's own state rather than a refusal of this
  request. A malformed event, a bound, and anything refused on this request's own terms stay
  final — retrying those spends a run's budget to be told the same thing four times. The wire
  itself still never resends once a person has read part of an answer; the loop above it, which
  owns the transcript, decides.

- **`b10x-harness tools` states which surface it answered for**, and under `flat` the published
  list and the catalogue name the same entries.

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

  *Superseded 2026-08-29: idempotency no longer asks; risk alone does. A `file_edit` is `medium`
  and asks exactly when a `file_write` does. The rest of this entry stands. See § Changed below.*
- **`--substrate-embedded` is a flag, not an option.** It demanded a value it then ignored; the
  README showed it bare and no test exercised it. It is now `bool` on `run` and `tools`, and an
  end-to-end test drives the embedded path.
- **Confinement the operator named and the machine cannot provide refuses the run by name.**
  `--substrate-embedded` over a directory not named `ws_…`, an embedded driver that does not open,
  or `--substrate <socket>` with no usable daemon behind it used to fall back to the read-only
  catalogue **silently** — the operator asked for write+exec, got a read-only run, and the model
  reported the task done. Each case now exits 1 with the reason. The embedded driver is opened
  once per run instead of twice.
- **The socket path works, and it was run.** Verified on 2026-08-29 against a daemon built from
  the pinned substrate revision (`f1cfc1c`) in a delegated user scope: `workspace_create`,
  `file_write`, `file_read`, a confined `/bin/echo` through `run` and a twelve-second
  `/bin/sleep` through `run` (`tests/live.rs`, ignored by default, `B10X_SUBSTRATE_SOCKET`).
  Four things stood between the client and that daemon, none of them the daemon's:
  - **`op` was missing, and then it was the wrong thing.** Every mutating body carried `input`
    alone, which the decoder refuses before it reads the input; `op` is a **caller-minted
    operation id** (`common.json#/$defs/operation-id`, 16–128 of `[A-Za-z0-9_-]`, an idempotency
    key the daemon reserves against the request's hash), not the operation's name — sending
    `"workspace.create"` there was refused for the `.`. The client mints one per mutation from
    time, process and a sequence.
  - **A read needs its query.** `GET …/files/{path}` without `?mode=file&offset=0&limit_bytes=…`
    is refused at `query`; the ceiling asked for is the daemon's own `workspace.read-limit-bytes`.
  - **The exec's output was never fetched.** The start answers the exec resource under `result`
    with its `id`; the client looked for `exec_id`, fell through to answering the start document,
    and the model would have got an exit code and no output. Both streams are now read
    (`…/output?stream=…&offset=0&limit_bytes=…`) and projected into the shape the embedded path
    answers: `stdout`, `stderr`, `stdout_truncated`, `output_complete`, `exit`.
  - **A program longer than ten seconds was reported unreachable.** `wait: true` holds the
    connection open until the exit, and the transport's read timeout was the probe's ten seconds;
    an exec now waits its own `timeout_ms` plus that.
  Before any of that, `Client::exec` posted `{workspace_id, argv}` and nothing else — no
  `sandbox`, no limits — so whether it ran unconfined was the daemon's choice. It now posts a body
  serialised from `substrate-wire`'s own `ExecStartInput` (`require: true`, `network: "none"`,
  the same limits the embedded path uses), built by one shared function so the two paths cannot
  drift, and refuses by name when the daemon states no capability snapshot. The snapshot is asked
  for **once per client** and held, and the CLI probes and serves with one client, so publication
  and admission read one document.
- **The wall-clock deadline is checked between the tool calls of one turn**, not only between
  turns, so a turn of several slow calls stops at the first one past the deadline instead of
  running all of them. And what is left on the clock is handed into each call —
  `ToolPort::call_within` → `Catalogue::invoke_within` → `Operations::run_within` — so a `run`
  is bounded by the smaller of its provider's own ceiling (600 s unconfined, 900 s confined) and
  the time the run has left, and its result says `timeout_ms` so a kill at the deadline is not
  read as the program's slowness. Before this a one-minute budget could not stop a fifteen-minute
  `cargo test`. It also has tests.
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

### Fixed

- **A run that failed now files what it spent, not only what it said.** A wire failure on turn
  twenty handed the shell its nineteen turns of conversation and none of their figures: the usage
  and cost of every turn that did happen scrolled past on stderr and then died with the process,
  and the session file — the only record left afterwards — showed the whole conversation at zero
  turns and no cost, so `b10x-harness sessions` listed a run that had been billed for nineteen
  turns as `0 turn(s)`. `AgentLoop::run_in` now takes a `RunLedger` beside the items — `usage`,
  `cost_micro_usd`, `turns` — and writes it on **every** exit path exactly as it writes the
  conversation back; `run` keeps its signature and `LoopError` keeps its three payload-free
  variants, so only a caller that lends a conversation pays for the extra argument.
  `transcript::Session::spent` folds it into the session in the failed arm of both `run` and
  `chat`. A run nobody could price still adds no cost rather than a zero. This is the rule
  `RunState::absorb_child` already applies to a delegate — a child that broke on turn four still
  bought three turns — reaching the top-level run, where the shell rather than the loop is the
  thing holding the record. The join is proved end to end by a new scenario both emulators serve,
  `fails-after-turn` — one whole turn with usage and a tool call, then a request answered `400`,
  which the retry rule treats as final so nothing waits out a back-off — against which
  `crates/harness-cli/tests/end_to_end.rs` asserts that a `run` on either wire and a `chat` line
  exit 1, leave a record that carries the bought turn and never a `finished`, and file a session
  holding two turns, the answered turn's usage and its cost, while a run nobody could price
  (`unauthorized` under a rate card) leaves that session unpriced rather than at zero.

- **A compaction can no longer fold a tool call away from its result**, or a reasoning item away
  from the call that follows it: the summary's fold boundary now falls only between whole turn
  groups. Both shapes were provider 400s on the turn after the compaction.
- **The summary turn's own request is one plain-text user item** with no tools, no tool blocks and
  no opaque items, instead of a replay of the folded conversation. On the `anthropic-messages` wire
  that replay was rejected twice over — an assistant-first message, and tool blocks with no `tools`
  — so every compaction there paid for a doomed turn.
- **`max_input_tokens` and `max_cost` are checked immediately after a compaction.** A summary
  turn's spend was absorbed but never tested against the ceilings, so a run overshot by a summary
  turn plus a full conversation turn.
- **A confined `file_read` no longer answers the read route's byte-ceiling prefix as though it
  were the whole file**: past the ceiling `lines.total` and `bytes` are `null`, `truncated` is
  `true`, and `route_ceiling_bytes` and a `note` say the lines past it are unreachable on that
  path. A `file_edit` of such a file is refused rather than writing the prefix back and deleting
  everything after it.
- **A tool call whose thread panics inside `Catalogue::invoke_batch` is a refusal naming the
  entry**; it no longer takes every sibling's answer with it.
- **`find` and `search` answer `depth_bound_reached`**, and both entries' descriptions name the
  directories they skip and the depth they stop at, so a bound is never read as an empty tree.
- **A CRLF file reads identically through the local and the confined provider**; a trailing `\r`
  quoted back to `file_edit` used to match nothing.
- **`find` refuses an empty `glob` by name** instead of answering an empty list.
- **The `batch-miscounted` warning says the port had already run the calls**, so each one in the
  group happens a second time — pure reads, and still stated.

- **A network blip twenty turns into a long run no longer throws the run away.** A turn whose
  stream broke after it had started speaking is attempted again — up to three times, pausing 0.5 s,
  1 s then 2 s, honouring Ctrl-C and the wall-clock budget inside the pause. Whatever streamed for
  that turn is announced as discardable (`turn-retried`) so a renderer can tell a person to
  disregard it, and the renderer prints exactly that. The wire still refuses to retry once it has
  emitted, because only the loop knows the conversation is unchanged by a failed turn; a failure
  the wire calls final is still final, and an error it already tried four times before the first
  byte goes up as final too, so a gateway that is down costs four requests and not sixteen.

- **A provider `error` event stopped losing the provider's own words.** Its `code` and `message`
  sit at the top level, not under `error`, and were read from the wrong place — every one of them
  arrived as `unknown` with no message, which also meant no retriable classification could ever
  fire.

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

### Known gaps

- **The transport half of the two wires is duplicated, and that is this change's real finding.**
  Bounded SSE framing, the retry rule, the witnessed sink that makes the retry rule safe, the
  back-off and the status mapping were copied from `harness-responses` unchanged, because none of it
  is vendor-shaped — it is *transport*-shaped, and the first wire could not tell the difference
  while it was the only one. A `harness-http` beneath both is what that argues for. It was
  deliberately not done here so that this change is the evidence rather than a guess acting on
  itself.
- **Nothing renews a subscription token.** The Anthropic route has now been contacted — a
  three-turn tool-using run against `https://api.anthropic.com/v1` on 2026-08-29, with a
  deliberately invalid token to the same endpoint answering `401` so the 200 is the credential's
  and not the endpoint's indifference (`STATUS.md` § *Subscription auth*). The ChatGPT/Codex route
  still has not been. Nothing here holds a refresh token or calls an authorization server, so a
  token nobody renews expires and the run fails by name.
- **Sub-agents, hooks, an MCP client, multimodal input and structured output are still not owned
  here** (`README.md` § *Not owned here*). Named because a comparison against other harnesses
  ranked them as the remaining gap; each is a decision about what this component owns rather than
  a defect in it, and the decision is pending.
- **The `verbs` surface's discovery cost is measured; the flat surface's is not.** 33–44% of tool
  calls went on discovery behind three verbs, across three live runs. What publishing flat costs
  or saves on a real provider — schema validation refusals, prompt-cache behaviour with seven tool
  definitions instead of three — is an experiment nobody has run yet, and both surfaces stay
  reachable from a flag so that it can be.

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
