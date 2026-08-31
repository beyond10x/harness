# harness

The b10x agent loop — ours, not a vendor's. It talks to LLM APIs directly and owns the cycle: one
turn out, tool calls back, results in, next turn out.

**[Read the public documentation](https://beyond10x.github.io/harness/)** for the quickstart,
execution model, safety boundary, operational guides and reference.

The source is publicly readable under the proprietary `LicenseRef-B10x-Proprietary` declaration.
Public visibility is not an open-source licence or a stability promise. Report a suspected security
issue through GitHub's private **Report a vulnerability** flow; see [`SECURITY.md`](SECURITY.md).

The problem it removes: driving a vendor's harness means booting a vendor binary whose only job is
to hold a loop, registering every tool through that vendor's mechanism, and living with whatever
budgets that vendor happens to enforce. Owning the loop makes tool names, budgets, cost accounting
and approval decisions ours.

It is deliberately small and carries no bridges to vendor binaries. **It depends on one other
component in `beyond10x` — [substrate](https://github.com/beyond10x/substrate), pinned by released
git tag — and on nothing that could embed it.** The arrow points inward — something else embeds
this, never the reverse.

## Where it sits

| direction | what |
|---|---|
| observed by | [metaharness](https://github.com/beyond10x/metaharness) — its `b10x` adapter launches `b10x-harness run` and reads the `--json` record. Observed, not driven: the published toolset already *is* the policy |
| confined by | [substrate](https://github.com/beyond10x/substrate) — embedded in-process, or over the daemon's socket |
| reaches models through | any OpenAI-Responses endpoint, e.g. [llmgw](https://github.com/beyond10x/llmgw), or any Anthropic-Messages one |
| mapped in | the private organisation architecture map |

## Status

**Pre-v1. Tagged `0.10.0` (2026-09-01).** The per-area state, with the exact next piece of evidence
each area is waiting for, is [`STATUS.md`](STATUS.md) — read that before believing anything here.

| area | state |
|---|---|
| `openai-responses` wire | implemented, streaming, pinned by contract |
| `anthropic-messages` wire | implemented, streaming, pinned by contract. Selected with `--wire`; the loop below cannot tell which it got |
| the loop: turns, tool round trips, approvals, budgets, cancellation | implemented |
| sub-agents (`delegate`), structured output (`answer`), skills (`skill`), hooks | implemented, opt-in per run; `provider_emulated` only — see [design 0002](docs/design/0002-sub-agents-structured-output-hooks.md) |
| command line (`run`, `chat`, `workflow`, `sessions`, `tools`, `app-server`, `events`) | implemented. Sessions are filed per run and resumable; the argv surface is pinned by contract |
| workflows (`workflow plan`, `workflow run`) | implemented, `provider_emulated` only — a step is a turn, a group is a scope, a boundary is a hook; see [design 0003](docs/design/0003-workflow-runner.md) |
| bridge mode (Codex app-server JSON-RPC over stdio) | implemented; **no real external bridge has ever driven it**, and no gate compares the two method inventories |
| substrate confinement, embedded | working, including execution — but `run` has been *published*, not yet *exercised* against a confined process |
| substrate over a socket | **working** — verified live 2026-08-29 against a daemon built from the pinned revision; see `STATUS.md` |
| live provider | first live run 2026-08-23. It found a real defect on turn 1 that the emulator could not: the whole workspace toolset was named illegally for that wire |
| embedding | **not started.** Nothing embeds this component yet |

Most evidence is `provider_emulated` and is never promoted to a claim about how a real provider
behaves. The wire contract pins are still emulator-derived.

## Build, test, run

The gate is **`cargo xtask gate`**. Green here is the bar for main.

| step | command |
|---|---|
| tests | `cargo test --workspace --locked` |
| format | `cargo fmt --all --check` |
| lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| provider-wire pins | `cargo xtask provider-contracts` |
| app-server profile pin | `python3 scripts/check-app-server-profile.py` |
| command-line argv pin | `cargo xtask cli-contract --self-test`, then `cargo xtask cli-contract` |
| absolute home paths | `python3 scripts/check-no-home-paths.py --self-test`, then `python3 scripts/check-no-home-paths.py` |
| brand | org-wide, from the atlas checkout: `bash ../atlas/scripts/check-org-brand.sh harness` |

Rust 1.97, edition 2024. The binary is `b10x-harness`.

```text
cargo run -p b10x-harness-cli -- run \
  --base-url https://llmgw.example/v1 \
  --model <alias> \
  --api-key-file <path> \
  --workspace . \
  --input "what does this workspace do?"
```

`--api-key-env <NAME>` is the alternative. **There is no ambient fallback** — the harness reads no
credential it was not pointed at, so a run can always be explained afterwards.

| flag | effect |
|---|---|
| `--json` | one event per line on stdout instead of prose. The first, `started`, carries `published_tools`, `operations` and — only when there is one — `withheld`, a tool this run declared and the machine would not admit, with the predicate that decided |
| `--prices <card>` | a JSON document of rates, with its own `source` and `as_of`; the record then carries the cost and the card that produced it |
| `--substrate <socket>` / `--substrate-embedded` | write and execute inside a confined workspace. Named and not available — an invalid one-component workspace name, a driver that does not open, no daemon at the socket — **refuses the run** (exit 1) rather than quietly running read-only |
| `--process-write-subtree <DIR>` | repeatable exact write authority for confined child processes. With none, `run` sees the adopted workspace read-only; this is separate from file-tool `--write-scope` |
| `--execution-path <direct\|metaharness>` | model-visible machine context describing whether this native loop was launched directly or as a metaharness arm; it grants no authority |
| `--cgroup-root` | the containing slice, when running inside a delegated cgroup |
| `--approve <mode>` | who decides a call above the ceiling. `auto` (the default) asks a person over `/dev/tty` when there is a terminal and stdin and stderr are one, and otherwise says so in one line and refuses; `prompt` asks or refuses the run by name; `deny` refuses and tells the model; `all` approves |
| `--yes` | the same as `--approve all`, and what every unattended invocation already says. It wins when both are given; it does not combine with `--approve-up-to` |
| `--approve-up-to <risk>` | raise the ceiling instead of changing who decides: `medium` lets `file_write` and `file_edit` through unasked, `high` lets `run` through too. Only calls **above** the ceiling reach the approver |
| `--surface <flat\|verbs>` | how the catalogue reaches the model. `flat` (the default) publishes every entry as its own tool with its own schema; `verbs` publishes `tool_search`/`tool_describe`/`tool_invoke` over it |
| `--session-dir <path>` / `--resume <id\|latest>` / `--no-session` | where the conversation is filed, which one to continue, or none at all |
| `--context-window <tokens>` | what the endpoint serves for this model. It bounds the request **and** drives compaction: the run compacts at 80% of it and frees to 50% |
| `--no-project-instructions` | leave the workspace's own `AGENTS.md`/`CLAUDE.md` out of the standing instruction. The environment block stays |

Exit status distinguishes the three outcomes a caller acts on differently: `0` the model answered,
`2` the run stopped for a named reason, `1` the harness could not run. A run refused **before** the
loop starts — a command line clap would not parse, a credential, a confinement, a session on the
other wire — exits `1` and, under `--json`, writes one line saying so:
`{"kind":"refused","reason":…}`. clap's own `2` is deliberately not used, because on this command
line `2` already means a run that happened and stopped.

### Sessions, resume and `chat`

Every `run` files its conversation under `$XDG_STATE_HOME/b10x-harness/sessions/<id>.json` —
outside the workspace, `0700`, written atomically — and says the identifier on stderr. It is
written whether the run answered or died, because a run that dies on turn 20 is exactly the one
whose nineteen turns must survive.

**It files what the run spent, too.** The usage, the cost and the turns of a run that broke on the
wire go into the session with its conversation: those turns were billed, their figures went past on
stderr while the run was alive, and once the process is gone the session file is the only place
still holding them. A failed run that showed nineteen turns of items and no tokens would read as a
failure that cost nothing.

```console
b10x-harness sessions                       # id, updated, model, turns — newest first
b10x-harness run --resume latest --input "and now the tests?"
b10x-harness chat --model <alias> --base-url …   # one line at a time, same session
```

Items are stored verbatim, opaque reasoning items included, so a resumed run replays what the
model already thought instead of paying for it again. **No credential and no instruction text is
written**: the instruction is derived from this run's catalogue, scope and project files, and
replaying under a stale one would give a run nobody could reproduce from its flags. A session
recorded on the other wire is refused by name before anything is sent — an opaque item may not
cross wires.

`--no-session` writes nothing at all, for an evaluation arm that must leave nothing on the machine
it ran on.

Under `--json` the identifier is the **last line of the record** —
`{"kind":"session","id":…,"path":…}` — so a driver reading the stream ends up holding what it needs
to continue the conversation, rather than having to parse it out of stderr.

**What is streamed is not what is stored.** On `anthropic-messages` a `thinking_delta` is the
model's visible thinking and it is rendered to stderr as it arrives; it is **not** persisted in a
session. What carries reasoning across a tool round trip is the opaque item the turn ends with,
exactly as it always was, and that is what a session holds.

### Running with confinement

The embedded driver serves execution only where the machine can confine one, and substrate decides:
its probe requires the process to be **inside** a delegated cgroup subtree whose root is
process-free and carries `cpu`, `memory` and `pids`. A process cannot move itself into one — the
kernel refuses a write across delegation domains — so it has to be *started* there:

```console
systemd-run --user --scope --property="Delegate=cpu memory pids" -- ./run.sh
```

and inside, pass the scope's containing slice as `--cgroup-root`. Without it the run has six tools
and no `run`; with it, seven. That is the toolset following the machine rather than a flag.

**And the run says which one it got.** The toolset is silent to the model on purpose — a tool it
cannot have is one it never plans around — but that silence used to reach everybody else too: six
entries where seven were asked for read exactly like six that were asked for, and a session whose
only legal route was starting a program hand-wrote files instead while the record said nothing. So a
program set that was **declared** and could not be admitted is now stated, naming the predicate that
decided:

```text
note: `run` is not published on this machine: `exec.argv-only` must be true and this machine says nothing. substrate states the exec facts only where its own cgroup probe passed, and that probe reads the probing process's `/proc/self/cgroup` and fails when it is outside the configured cgroup root — the embedded driver probes *this* process, and a login shell sits in `user.slice/user-N.slice/session-M.scope`, a sibling of the `user@N.service` manager scope, so the same machine answers differently under `systemd-run --user --scope`.
```

One line on stderr before the run, a `withheld` array in `b10x-harness tools`, and a `withheld`
field on the `started` event under `--json`. A run that declared **no** programs states nothing:
absence stays absence, and a read-only run is owed no sentence about a tool it never wanted. The
same record covers `file_write` and `file_edit` when a confinement was named and the machine states
no `workspace.guarded-io`.

The workspace is **adopted, not created**: `--workspace` is the tree, its parent becomes substrate's
root, and reads and writes land in the same place. Its directory name must be one non-empty path
component of ASCII letters, digits, `_` or `-`, not `.` or `..` and not beginning with `-`. A name
outside that grammar refuses the run rather than quietly running read-only:

```text
error: `--substrate-embedded` cannot adopt `-project`: the directory name must be one non-empty ASCII path component using letters, digits, `_` or `-`, and it may not begin with `-`.
```

`--toolchain rust` mounts the operator's `~/.rustup` read-only inside the sandbox and points
`CARGO_HOME` at **`<workspace>/.cargo`** — never at the operator's `~/.cargo`, which holds a
registry credential. Nothing seeds that directory: a confined build has no network, so the caller
copies the package cache the task needs (`registry/` from `~/.cargo`) into `<workspace>/.cargo`
before the run, or the first `cargo build` fails inside cargo looking for a crate it cannot fetch.

`--toolchain go` mounts the `GOROOT` named by the operator, or the installation containing the
first `go` on `PATH`, read-only at `/toolchain/go`. `GOPATH`, the module cache and the build cache
all live under the workspace; `GOENV=off` excludes the operator's Go configuration, and the
sandbox's unshared network prevents module lookup from reaching a proxy. A build can therefore use
the standard library and modules already present in the workspace, but it cannot inherit cached
private modules or reach a proxy.

`--toolchain auto` resolves the built-in Rust, Go, Taskfile, npm and Yarn YAML providers from
root-relative markers without executing a compiler or task runner. Taskfile public tasks and root
package scripts become enum arguments of one fixed `taskfile_run`, `npm_run` or `yarn_run` tool;
they do not inflate the prompt with one tool per task. Language providers also contribute generic
roles when every active provider implements the role. `--toolchain-spec FILE` adds an operator
provider explicitly and is never discovered from the workspace. The catalogue is frozen before
turn one, and internally admitted programs cannot be reached through raw `run` arguments.

Every standing input is now a typed context layer. `--instructions-file` contributes an
operator-trust layer beside the harness guidance rather than replacing it. `b10x-harness context
show` prints the body-free provenance manifest; add `--body` only when the exact prompt text is
needed. Audit fields such as the digest, byte count, capture time, and cache class are recorded but
not sent to the model.

## What the model sees

One [catalogue](crates/harness-tools/src/catalogue.rs) whose entries are named by neutral
operations, published under one of two surfaces.

| entry | operation | what it does | needs |
|---|---|---|---|
| `file_read` | `file.read` | one file, or a window of it: `offset`/`limit` in lines, answered `cat -n`-numbered with `lines: {from, to, total}` | — |
| `dir_list` | `dir.list` | one directory, not recursive, 500 entries | — |
| `search` | `search` | text across the tree: literal or `regex`, filtered by `glob`, with `context` lines either side | — |
| `find` | `find` | every path matching a glob, in one call instead of one `dir_list` per level | — |
| `file_write` | `file.write` | one file, whole | a confined workspace |
| `file_edit` | `file.edit` | one exact piece of text, which must appear exactly once | a confined workspace |
| `run` | `shell` | an argv over a **declared** program set — never a shell | a delegated cgroup |

For `run`, the declaration matches `argv[0]`: the root executable Harness starts. Compilers,
linkers and other descendants it starts are not matched again. They remain in the same sandbox,
cgroup limits, no-network namespace and workspace boundary, and timeout or cancellation kills the
whole process tree.

Four entries with no backend, six with a confined workspace, seven inside a delegated cgroup. The
catalogue is what the machine can perform, and a tool the machine cannot confine is one no surface
ever lists — and one that was *asked for* and could not be admitted is reported beside the list it
is missing from, rather than only being missing (see [*Running with confinement*](#running-with-confinement)). Reads are bounded to the workspace root and every path is re-checked after
canonicalization — including each entry a search walks into — so a symlink inside the workspace
cannot be used to read outside it.

**`--surface flat` is the default**: each entry is published as its own tool, with its own schema,
so the provider validates arguments and nothing has to be discovered first. The surface was three
verbs — `tool_search`, `tool_describe`, `tool_invoke` — and the reason for them was neutral names
across harnesses, since an evaluation compares arms that each spell one act differently (`Bash`
here, `run` there, `Write` and `workspace_write`). That reason survives without the indirection:
the **entry names are the vocabulary**, and `harness_tools::operation_of` maps them for a reader of
a finished run. What did not survive is the cost — across three live runs **33–44% of every tool
call was `tool_search` or `tool_describe`**, and `tool_invoke.arguments` was an untyped object no
provider could check.

`--surface verbs` publishes the three verbs over the same catalogue and is fully supported:
metaharness serves that surface over MCP, and an arm comparing the two asks for it by name. Under
it, a model that calls an entry by its bare name is routed to it and the waste is warned about
(`unpublished-tool-routed`) rather than costing a dead turn.

With no `--substrate` or `--substrate-embedded`, nothing the run can call changes a file or starts
a process. **That is a fact about the machine, not a promise this README makes to the model**: the
standing instruction states no effects of its own and names only what this run actually has. It
used to name three tools that no longer existed and assert that none of them could write — and a
measured run given a write-and-execute catalogue believed it, read two files, changed nothing, and
reported the task done.

The instruction also carries an **environment block** — the absolute workspace path, the OS and
architecture, today's UTC date, and the git branch, read from `.git/HEAD` rather than by spawning
`git` — and the workspace's own `AGENTS.md` (else `CLAUDE.md`), bounded at 32 KiB and said so in
words. `--no-project-instructions` leaves the project's words out; the environment block stays.

## Budgets and cost

`max_turns`, input and output token totals, a wall-clock deadline and — with `--prices` — a spend
ceiling are counted here, so they are enforced here. A bound that *cannot* be enforced is
[refused by name](crates/harness-loop/src/budget.rs) rather than accepted and ignored: a spend
ceiling on a run with no rates to measure it against stops the run before the first request instead
of pretending.

No provider on this wire returns a price. Rates are declared in a `--prices` card and never
compiled in — a table baked into this binary would be numbers nobody could date, wrong silently the
first time one moved. Without a card the run reports tokens and no price, and a model the card does
not list is **warned about by name** rather than reported as free.

Approval is a **blocking call**, not a protocol round trip that can land after the effect. What
asks is derived from the call, never declared by the tool: the loop reads the envelope of the
catalogue entry the call resolves to — the entry's, not the surface's — and asks when its risk is
above the run's unattended ceiling (default low, `--approve-up-to` raises it). Risk alone decides:
an idempotency clause used to ask about every `file_edit` whatever the ceiling, which pushed an
unattended run toward rewriting whole files when the narrower edit was the safer act.

**The library's default approver is `DenyAll`.** The command line's is `--approve auto`: a person
over `/dev/tty` when there is one to ask, and a refusal — stated in one line before the run — when
there is not. `--yes` (`--approve all`) is the declared unattended run.

## Sub-agents, structured output, skills, hooks

Four opt-in extensions under one rule: **nothing reaches a
tool without the gate, nothing widens what a turn admits, and nothing refuses silently.** The
argument is [design 0002](docs/design/0002-sub-agents-structured-output-hooks.md); each is off
unless a run asks for it.

| what | how it is spelled | what it is |
|---|---|---|
| structured output | `--output-schema <FILE>` | the schema is published as a tool named `answer` that the model calls to finish; its arguments are the answer. **Stdout is that JSON and nothing else**, written once when the run completes, so the command composes — except under `--json`, where stdout is the event record and carries no bare answer line: the answer is then the **last** `answered` event before a `finished` whose `stop.kind` is `completed`, because a `stop` hook can withdraw an earlier one and the run answers again. A model that ends in prose is told once to call it **and the turn that ask opens is held to that tool at the provider** — one turn per run, never any other; if it still does not, the run stops `unstructured` and exits 2 — never a success status over prose |
| sub-agents | `--delegate` (`--delegate-turns N`, default 20; `--delegate-parallel N`, default 4) | a tool named `delegate`: a second loop runs to completion inside the tool call over a **fresh** conversation, with the same tools, the same approver, the same hooks, the same cancel and a share of the parent's remaining budget. The parent reads one result — `{stop, turns, text}` — never the child's transcript. Depth one: a delegate cannot delegate. **Neighbouring `delegate` calls of one turn run side by side** — each child gets a fork of the model and tool ports, while the approver, the hooks and the record stay single and are asked from the run's own thread. `--delegate-parallel 1` runs them one at a time |
| skills | `--skills-dir <DIR>` or `--plugin-dir <DIR>` | a tool named `skill` returns one operator-supplied instruction document by name. Descriptions are present in the standing instruction; bounded bodies are loaded before the run and returned only on demand. The loop performs no filesystem discovery and a delegate receives the same immutable set. |
| hooks | `--hooks <FILE>` | the operator's own programs, run as an argv (never a shell) at three moments: `before-call` (after approval; exit 2 refuses the call, a hook that fails refuses it too), `after-call` (a note the model reads beside the result), `stop` (exit 2 keeps the run working with the reason, at most three times). Named on the command line, **never discovered in the workspace** |

None is a catalogue entry or touches `harness-wire`: `answer`, `delegate` and `skill` are tools the
**loop** owns, resolved before the tool port ever sees a call, and a hook is a port like the approver,
with the process-running half in the shell. A delegate's tool calls meet exactly the gate the
parent's do; a hook can refuse what the gate allowed and can allow nothing the gate refused.

Running delegates side by side changes how long a turn takes and nothing else. It is permitted only
when the reachable surface is non-mutating and needs no approval and no hook is attached; otherwise,
when a port will not fork, or when the remaining token budget will not divide, the same delegates
run in order and reach the same results in the same order.

Provider-native structured output (constrained decoding on the wire) and delegate *trees* remain
later milestones. The loop now validates every `answer` locally against its declared JSON Schema.

## Workflows

A workflow is a document this loop walks itself — [`harness-flow`](crates/harness-flow/src/lib.rs)'s
notation, a DAG of sub-trees whose edges join siblings only, bound to the loop by
[design 0003](docs/design/0003-workflow-runner.md). No `metaharness` and no `protocol` process is
involved: the loop sees the whole graph, so a section stays warm across its steps and a retreat
re-enters a scope instead of paying for its context again.

```console
b10x-harness workflow plan --flow flow.yaml                 # validate, print the plan; contacts nothing
b10x-harness workflow run  --flow flow.yaml --input "…"     # walk it, with the run flags chat takes
```

`plan` takes only `--flow` and `--max-attempts` — no endpoint, no credential — so *does this
document validate, and what runs in what order* is free, exactly as `tools` is. `run` flattens the
same options `run` and `chat` do, plus `--flow <FILE>` (`.yaml`/`.yml` or `.json`, decided by
extension), `--input <TEXT>` (the task, given to every step beside its own prompt) and
`--max-attempts <N>`, which overrides every `repeat.max` — the root's included — for a document
that carries none.

**A step is one turn, one call, or a handoff.** Every model step runs under an output schema the runner
derives — the model never sees a schema file — and finishes by calling `answer` with
`outcome: passed` or `outcome: failed`,
an optional `note`, and `gives` when its enclosing group promised names. `gives` is the only thing
that crosses a group boundary, so a group that promised `specification_id` and never answered with
one fails by the notation's own rule rather than by a new one — once, and without a retreat, since
a section that came out clean and still did not produce it buys the same answer again. A budget stop, or prose after the
nudge, is a failed step. A wire failure is nobody's failed step: it aborts the flow, because a walk
that recorded a network blip as `failed` would misreport the plan. A step whose `run` says
`kind: command` is not a turn at all: its argv is one `run` call through the same gate a model's
call meets — published, approver, `before-call`, tool, `after-call` — filed into the section's
conversation, exit `0` passed and everything else failed by name.

A step whose `run.kind` is `operator` is the boundary a model must not cross: its non-empty
`prompt` becomes the reason on one terminal `flow-paused` event, the step is counted as reached
rather than failed, and the process exits `0`. Nothing after it is called skipped, no open group
is left or retreated, no session is invented, and that step reaches neither budget, scope, tool,
approval, call hook nor provider. An unknown kind and an operator step without a prompt are refused
by `workflow plan`, so neither can silently become a model turn.

**A group is a conversation.** Steps sharing a scope continue the same items; a step in another
group starts from the handoffs of the siblings that came out **clean** — rendered as *"Earlier
sections established:"* — and nothing else, so a sibling's transcript never crosses and a result
nobody accepted is not what the rest of the walk is built on. **A retreat is `Repeat`**: a group that
did not come out clean is re-entered from nothing but those handoffs, up to the bound the document
wrote down.

**A section boundary can be refused.** `--hooks` learns a fourth point, `transition`, asked before a
group is entered and again after it leaves, under the same rules as the other three — declared,
never discovered, narrowing only. Exit 2 at `enter` skips the section as failed, exit 2 at `leave`
on a clean attempt forces a retreat, and a hook that cannot answer fails closed; the protocol is in
the [workflows guide](website/docs/guides/workflows.md).

**One session per `(scope, attempt)`**, id `<flow-run-id>` then every open section on the way down
with the attempt it is on — `….root.2.implement-to-review.3.verify.1` — filed with what it cost as
the scope closes. `--no-session` writes nothing; `--resume` is refused, because a flow names its
own sessions and resuming a *flow* is a later milestone. Exit status reads as it does everywhere
else: `0` the flow came out clean or is awaiting an operator, `2` it finished and did not — a failed step, a skipped or
exhausted section, a cancelled run — `1` refused before it started, or aborted mid-step.

**What stays outside: the governor.** Guards, evidence and transition budgets are
engineering-protocols' engine and stay there — this harness embeds nothing above it (invariant 2),
and a runner that evaluated a gate would be a second protocol implementation with none of the
conformance suites behind it. `protocol workflow flow` projects `adp/default/2` into this notation
and says what the projection is: an ordering, not a government. Nor is this an eval arm — a run here
moves the sequencer, so it is a different experiment, and where it is measured against the driven
arm it is measured as cost, tokens and wall-time under the **same** governor program.

## Three shells, one loop

| shell | what it is |
|---|---|
| embedded | `harness-loop` as a library; tools bound in-process, no IPC |
| command line | `b10x-harness run` and `b10x-harness chat`, over a workspace |
| bridge | `b10x-harness app-server`, a process speaking the Codex app-server JSON-RPC format |

The seam that makes this possible is [`ToolPort`](crates/harness-wire/src/port.rs): in-process it
is a direct call, under a bridge it is a callback over the wire, and the loop cannot tell.

Bridge mode declares the client's **operation-tools** profile, not the plain
`codex-app-server-stdio-v2` — that one is the *stable* profile and admits no dynamic tools, so
declaring it while emitting tool frames yields a server that looks compatible and fails at the
first tool call. The client therefore has to negotiate `experimentalApi`; registering tools without
it is refused by name, and a text-only thread works either way. `thread/resume` and `turn/steer`
are refused by name rather than answered with a silent success.

## Wires

| wire | endpoint | state |
| --- | --- | --- |
| `openai-responses` | `POST {base}/responses`, streaming | implemented |
| `anthropic-messages` | `POST {base}/messages`, streaming | implemented |

`b10x-harness run --wire anthropic-messages` picks the second one. The wire is a branch in exactly
one function; below it the loop holds a `ModelPort` and cannot tell which projection it got, which
is the whole reason a second wire cost a second projection instead of a second loop.

A wire crate is now **only** the projection — the request body, the stream decoder, the endpoint's
path and its header names. Everything under that is
[`crates/harness-http`](crates/harness-http/src/lib.rs): bounded SSE framing, the retry rule, the
back-off, the witnessed sink that stops a turn being resent once a person has read part of it, and
the HTTP status mapping. That half was written for the first wire and copied unchanged by the
second, which is what proved it was transport-shaped rather than vendor-shaped. The two wires
configure it identically but for one thing, and that one thing is checked: the first route ends its
stream with `data: [DONE]` and the second has no sentinel at all.

Turns are **stateless**: nothing is retained on the far side and the whole conversation is replayed
each time. Reasoning — `reasoning` items on one wire, `thinking` and `redacted_thinking` blocks on
the other — is carried verbatim as [`Item::Opaque`](crates/harness-wire/src/item.rs) tagged with the
wire that produced it, so the model keeps its own chain of thought across a tool round trip and
cannot have one provider's blob replayed into another's. Replaying one into a wire that did not
produce it is a typed refusal naming both wires, not a silent drop.

While a turn is streaming, the reasoning the provider chooses to send — `reasoning_summary_text`
deltas on one wire, `thinking_delta` blocks on the other — reaches a reader on **stderr** as it
arrives, so a long think is not a silent minute. It is shown and let go: what crosses a tool round
trip, and what a session stores, is the opaque item, never the streamed text.

Both wires are exercised by the **same** loop suite over a real socket: the same case names against
the same scenario names, with a test that fails if either side grows a case the other lacks. A
second comparison covers the half beneath them —
[`crates/harness-messages/tests/transport.rs`](crates/harness-messages/tests/transport.rs) reads
what each wire asks of `harness-http` and fails on any difference but the framing, so a wire that
quietly doubled its attempts or halved a timeout is caught by a test rather than by a person
reading two files side by side.

## Layout

| crate | owns |
|---|---|
| `crates/harness-wire` | neutral values plus `ModelPort`, `ToolPort` and `BearerSource`. No I/O, no clock, no vendor field name. It defines the credential types; it reads and sends none |
| `crates/harness-credential` | credential sources that read exactly what a caller pointed them at. Nothing vendor-shaped: how a fetched credential is *presented* belongs to the wire |
| `crates/harness-http` | the transport half of a wire: bounded SSE framing, the retry rule and its back-off, the witnessed sink, the status mapping and the one blocking `POST`. No vendor name, field name or header name |
| `crates/harness-responses` | the Responses projection: its request body, its stream decoder, its three conversation headers |
| `crates/harness-messages` | the Messages projection: its request body, its content-block decoder, and the two header names one secret travels under |
| `crates/harness-loop` | the loop: turn assembly, tool round trips, approvals, budgets, cancellation; the three tools it owns itself (`answer`, `delegate`, `skill`) and the hook port |
| `crates/harness-flow` | the workflow notation `workflow run` walks: a DAG of sub-trees, validated before anything runs, a group as a context scope, and a boundary a caller can refuse |
| `crates/harness-substrate` | a client of the substrate wire: what this machine can confine, and the tools that answer |
| `crates/harness-tools` | one catalogue, published flat or under three verbs |
| `crates/harness-app-server` | the Codex-format JSON-RPC server, and the wire-backed `ToolPort` |
| `crates/harness-cli` | the `b10x-harness` binary, the terminal approver, the hook runner, session transcripts and the environment block |

| path | holds |
|---|---|
| `contracts/provider-wires/` | the exact request sent and the exact stream accepted, per dated pin |
| `contracts/app-server-profile/` | the JSON-RPC subset served, per dated pin |
| `contracts/cli/` | the argv surface accepted, per dated pin — generated from clap, never written by hand |
| `docs/design/` | component design documents |
| `crates/harness-xtask` | the executable gate and the independent provider and CLI contract validators |
| `scripts/` | compatibility entry points and the two legacy checks not yet touched by this change |

## Evidence

Every contract pin is checked **from both directions** — an independent checker verifies the
manifest against its fixtures, and a Rust test in the owning crate verifies the code produces those
bytes or holds those constants. The provider and CLI checkers run through `cargo xtask`; the
app-server profile checker remains the existing Python program. Neither half is sufficient alone.

These suites drive real processes over real sockets and pipes:

| suite | drives |
|---|---|
| `crates/harness-responses/tests/provider_emulated.rs` | the first wire's client and the loop against a local HTTP endpoint |
| `crates/harness-messages/tests/provider_emulated.rs` | the same suite, pointed at the second wire |
| `crates/harness-cli/tests/end_to_end.rs` | the shipped binary over a real workspace: both surfaces, both wires, sessions, resume, `chat`, the approver and a refused command line |
| `crates/harness-cli/tests/bridge_mode.rs` | the shipped binary driven as a bridge would drive it |
| `crates/harness-cli/tests/workflow.rs` | the shipped binary walking a flow document: one session per section, a retreat to its bound, and a hook refusing a transition |

## Not owned here

- **No substrate confinement claim.** This harness's effects are exactly what its published toolset
  admits, and nothing constrains it further.
- **No live-provider conformance.** One live run has happened; the pins are still emulator-derived.
- **No MCP client and no multimodal input.** Of the five gaps the comparison against other
  harnesses ranked (`docs/reviews/2026-08-29-sota-comparison.md`), sub-agents, structured output
  and hooks landed the same day (design 0002); these two stay decisions about what this component
  owns. An MCP client would make the loop a client of a protocol whose tools nothing here confines
  — metaharness is the MCP side of this family — and an image item is a new neutral value on both
  wires that nothing measuring this harness has asked for.

  **Reading a vendor's on-disk file format is not the same act as becoming a client of its
  protocol**, which is why skills and agents could be added (`--skills-dir`,
  `--agents-dir`, `--plugin-dir`, in the layout Claude Code writes) while an MCP client stays
  refused. A file format has no reach: nothing opens
  a socket, nothing gives a third party a say in what a run may do, and the bytes are read once,
  before the run starts, out of a directory the operator named. A protocol has all three.

- No realtime media or provider-side sessions. Sessions here are **this harness's own file on this
  machine**: nothing is retained on the far side and the whole conversation is replayed every turn
  (invariant 4).
- No hosted service, no admission transport, no durable store.

## Read more

- [`STATUS.md`](STATUS.md) — per-area state and the exact next evidence each area waits for.
- [`ROADMAP.md`](ROADMAP.md) — the outcome roadmap; a phase advances only when its exit evidence
  exists.
- [`CHANGELOG.md`](CHANGELOG.md) — what changed and what each change cost to learn.
- [`docs/design/0001-tool-envelope-and-substrate-confinement.md`](docs/design/0001-tool-envelope-and-substrate-confinement.md)
- [`docs/design/0002-sub-agents-structured-output-hooks.md`](docs/design/0002-sub-agents-structured-output-hooks.md)
- [`AGENTS.md`](AGENTS.md) — working agreements and invariants for anyone changing this repo.
